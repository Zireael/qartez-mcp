//! Parser interface backed by thread-local workers.
//!
//! This module wraps the newer ParserWorkers for backward compatibility.
//! The workers are lazy-loaded per language and avoid the Mutex bottleneck.

use crate::index::parser_workers::ThreadLocalParserWorkers;

/// Re-exports ThreadLocalParserWorkers as ParserPool for API compatibility
pub type ParserPool = ThreadLocalParserWorkers;
