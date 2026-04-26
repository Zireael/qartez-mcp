//! Thread-local lazy-loaded parser workers.
//!
//! Replaces single Mutex<Parser> bottleneck with per-language workers that are:
//! - Created lazily on first use (no upfront grammar cost)
//! - Per-thread to avoid locking contention
//! - Evicted after idle timeout (configurable)

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tree_sitter::Parser;

use crate::error::{QartezError, Result};
use crate::index::languages;
use crate::index::symbols::ParseResult;

/// Configuration for parser workers
#[derive(Debug, Clone)]
pub struct ParserWorkerConfig {
    /// TTL for idle workers before eviction (default: 15 minutes)
    pub parser_idle_ttl: Duration,
}

impl Default for ParserWorkerConfig {
    fn default() -> Self {
        Self {
            parser_idle_ttl: Duration::from_secs(15 * 60),
        }
    }
}

/// State of a parser worker
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerState {
    /// Worker created, no language loaded yet
    Idle,
    /// Currently loading language
    LoadingLanguage,
    /// Actively parsing
    Parsing,
    /// Worker is being evicted (dropping language)
    Evicting,
}

/// A parser worker for a specific language
pub struct ParseWorker {
    /// Language extension this worker handles (e.g., "rs", "ts", "js")
    pub language: String,
    /// Current state machine state
    pub state: WorkerState,
    /// Last time this worker was used (for TTL tracking)
    pub last_used: Instant,
    /// The actual tree-sitter parser (loaded lazily)
    parser: Parser,
}

impl ParseWorker {
    /// Create a new idle worker for a language
    pub fn new(language: &str) -> Self {
        Self {
            language: language.to_string(),
            state: WorkerState::Idle,
            last_used: Instant::now(),
            parser: Parser::new(),
        }
    }

    /// Ensure language is loaded in the parser (lazy load)
    pub fn ensure_language(&mut self, ext: &str) -> Result<()> {
        if self.state == WorkerState::Idle {
            self.state = WorkerState::LoadingLanguage;
        }

        if self.state != WorkerState::LoadingLanguage {
            return Ok(());
        }

        let support = languages::get_language_for_ext(ext)
            .or_else(|| {
                let filename = format!("file.{}", ext);
                languages::get_language_for_filename(&filename)
            })
            .ok_or_else(|| QartezError::Parse {
                path: "".to_string(),
                message: format!("unsupported extension: {}", ext),
            })?;

        let lang = support.tree_sitter_language(ext);
        self.parser.set_language(&lang).map_err(|e| QartezError::Parse {
            path: "".to_string(),
            message: format!("failed to set language: {}", e),
        })?;

        self.state = WorkerState::Idle;
        Ok(())
    }

    /// Parse a source file (caller must ensure_language first)
    pub fn parse(&mut self, source: &[u8]) -> Result<tree_sitter::Tree> {
        self.last_used = Instant::now();
        self.state = WorkerState::Parsing;

        let tree = self
            .parser
            .parse(source, None)
            .ok_or_else(|| QartezError::Parse {
                path: "".to_string(),
                message: "tree-sitter parse returned None".to_string(),
            })?;

        self.state = WorkerState::Idle;
        Ok(tree)
    }

    /// Check if worker has been idle too long
    pub fn is_idle_too_long(&self, ttl: Duration) -> bool {
        self.state == WorkerState::Idle && self.last_used.elapsed() > ttl
    }
}

/// Thread-local parser worker pool
pub struct ParserWorkers {
    /// Workers indexed by language extension
    workers: HashMap<String, ParseWorker>,
    /// Configuration
    config: ParserWorkerConfig,
}

impl Default for ParserWorkers {
    fn default() -> Self {
        Self::new()
    }
}

impl ParserWorkers {
    pub fn new() -> Self {
        Self {
            workers: HashMap::new(),
            config: ParserWorkerConfig::default(),
        }
    }

    /// Get or create a worker for the given file extension
    pub fn worker_for(&mut self, ext: &str) -> Result<&mut ParseWorker> {
        // Get or create worker for this language
        let worker = self.workers.entry(ext.to_string()).or_insert_with(|| {
            ParseWorker::new(ext)
        });

        // Ensure language is loaded (lazy initialization)
        worker.ensure_language(ext)?;

        Ok(worker)
    }

    /// Parse a file using the appropriate worker
    pub fn parse_file(&mut self, path: &Path, source: &[u8]) -> Result<(ParseResult, String)> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // Determine language from extension first, then filename
        let lang_ext = if ext.is_empty() {
            if let Some(support) = languages::get_language_for_filename(filename) {
                // Use filename's primary extension
                let exts = languages::supported_extensions();
                exts.iter()
                    .find(|e| {
                        languages::get_language_for_ext(e)
                            .map(|s| s.language_name() == support.language_name())
                            .unwrap_or(false)
                    })
                    .map(|e| e.to_string())
                    .unwrap_or_default()
            } else {
                String::new()
            }
        } else {
            ext.to_string()
        };

        if lang_ext.is_empty() {
            return Err(QartezError::Parse {
                path: path.display().to_string(),
                message: format!("unsupported file: {}", filename),
            }.into());
        }

        self.parse_with_key(path, source, &lang_ext)
    }

    fn parse_with_key(&mut self, path: &Path, source: &[u8], lang_ext: &str) -> Result<(ParseResult, String)> {
        // Get or create worker for this language
        let worker = self.worker_for(lang_ext)?;

        // Parse the source
        let tree = worker.parse(source)?;

        // Get language support for extraction
        let support = languages::get_language_for_ext(lang_ext)
            .or_else(|| {
                let filename = format!("file.{}", lang_ext);
                languages::get_language_for_filename(&filename)
            })
            .ok_or_else(|| QartezError::Parse {
                path: path.display().to_string(),
                message: format!("unsupported language: {}", lang_ext),
            })?;

        let result = support.extract(source, &tree);
        Ok((result, support.language_name().to_string()))
    }

    /// Clean up idle workers past TTL
    pub fn evict_idle_workers(&mut self) {
        let ttl = self.config.parser_idle_ttl;
        self.workers
            .retain(|_, worker| !worker.is_idle_too_long(ttl));
    }
}

/// Thread-safe wrapper for parser workers
pub struct ThreadLocalParserWorkers {
    inner: Arc<Mutex<ParserWorkers>>,
}

impl Default for ThreadLocalParserWorkers {
    fn default() -> Self {
        Self::new()
    }
}

impl ThreadLocalParserWorkers {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ParserWorkers::new())),
        }
    }

    /// Parse a file (uses internal locking, but workers are per-language)
    pub fn parse_file(&self, path: &Path, source: &[u8]) -> Result<(ParseResult, String)> {
        let mut workers = self.inner.lock().map_err(|e| QartezError::Parse {
            path: path.display().to_string(),
            message: format!("failed to acquire parser lock: {}", e),
        })?;
        workers.parse_file(path, source)
    }

    /// Clean up idle workers
    pub fn evict_idle_workers(&self) {
        if let Ok(mut workers) = self.inner.lock() {
            workers.evict_idle_workers();
        }
    }
}