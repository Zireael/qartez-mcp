# Architecture

## Pattern Overview

**Overall:** Pre-computed knowledge graph server with MCP (Model Context Protocol) interface

**Key Characteristics:**
- **Agent-first design:** All data structures and responses optimized for LLM consumption, not human readability
- **Pre-computation over live scanning:** Index, PageRank, co-change, and complexity are computed once and queried from SQLite
- **Quad-signal impact analysis:** Fuses PageRank importance, static blast radius, git co-change affinity, and cyclomatic complexity
- **Local-first, zero cloud dependency:** Everything runs on the machine, no network required (optional semantic feature downloads a local embedding model)
- **Progressive tool disclosure:** Tools organized into tiers (core / analysis / refactor / meta) unlockable on demand
- **Modification guard:** Separate binary hooks into Claude Code PreToolUse to block edits on load-bearing files unless impact analysis runs first

## Layers

**CLI and Config Layer:**
- Purpose: Parse command-line arguments, detect project roots, resolve configuration
- Location: src/cli.rs, src/config.rs, src/cli_runner.rs, src/error.rs
- Contains: Cli struct (clap-derived), Config struct (root detection, DB path, git depth), OutputFormat enum, Command enum (19 CLI subcommands)
- Depends on: clap, serde, serde_yaml, toml_edit
- Used by: main.rs (both MCP-server and CLI-subcommand paths)

**Library Root:**
- Purpose: Re-export all public modules and gate feature-gated modules behind cfg attributes
- Location: src/lib.rs
- Contains: Public mod declarations for cli, cli_runner, config, error, git, graph, guard, index, lock, readiness, server, storage, str_utils, test_paths, toolchain, watch; feature-gated benchmark and embeddings
- Depends on: All internal modules
- Used by: All binaries (main.rs, bin/guard.rs, bin/setup.rs, bin/benchmark.rs)

**Indexing Engine:**
- Purpose: Parse source files via tree-sitter, extract symbols/imports/references, compute structural shape hashes, full and incremental indexing
- Location: src/index/ (mod.rs, walker.rs, parser.rs, parser_workers.rs, symbols.rs, fingerprint.rs, languages/ with 37 language adapters)
- Contains: IndexedFile, ChangeSet, ParserPool, per-language LanguageAdapter trait impls, workspace fingerprint computation, file walker
- Depends on: tree-sitter core + 37 language grammars, rayon, ignore
- Used by: main.rs (background indexing), cli_runner.rs (CLI indexing), watch.rs (incremental re-indexing)

**Graph Analysis Layer:**
- Purpose: PageRank, blast radius (BFS), Leiden community detection, boundary enforcement, security rules, architecture wiki generation
- Location: src/graph/ (mod.rs, pagerank.rs, blast.rs, leiden.rs, boundaries.rs, security.rs, wiki.rs)
- Contains: compute_pagerank, compute_symbol_pagerank, BlastComputer, LeidenClustering, BoundaryEngine, SecurityEngine, WikiConfig, render_wiki
- Depends on: Storage layer, rayon
- Used by: main.rs (post-index), cli_runner.rs, tool handlers (impact, boundaries, security)

**Git Mining Layer:**
- Purpose: Co-change pairs, complexity trends over git history, bus-factor analysis
- Location: src/git/ (mod.rs, cochange.rs, diff.rs, trend.rs, knowledge.rs)
- Contains: CoChangeConfig, analyze_cochanges, DiffImpactAnalyzer, TrendAnalyzer, KnowledgeAnalyzer
- Depends on: git2 (libgit2 bindings), Storage layer
- Used by: main.rs (post-index), tool handlers (cochange, trend, knowledge, diff_impact)

**Storage Layer:**
- Purpose: SQLite schema, read/write operations, maintenance (vacuum, checkpoint, FTS optimize), startup telemetry
- Location: src/storage/ (mod.rs, schema.rs, models.rs, read.rs, write.rs, maintenance.rs)
- Contains: open_db, create_schema, SymbolInsert and other row structs, query/mutation helpers, startup_telemetry, maintenance commands
- Depends on: rusqlite (bundled SQLite with FTS5), serde_json
- Used by: All layers that persist or query data

**MCP Server Layer:**
- Purpose: rmcp server handler -- dispatches tool calls, manages tool tiers, serves workflow prompts
- Location: src/server/ (mod.rs, tools/ with 42 handler modules, prompts.rs, tiers.rs, cache.rs, helpers.rs, params.rs, overview.rs, treesitter.rs, mcp_instructions.md)
- Contains: QartezServer, ToolRouter composition, TierManager (progressive disclosure), 5 workflow prompt templates
- Depends on: rmcp, Storage layer, Graph layer, tokio
- Used by: main.rs (server entry point)

**Readiness Subsystem:**
- Purpose: Track indexing lifecycle state (ColdStart, Indexing, Ready, PartialReindex), signal queryability
- Location: src/readiness.rs
- Contains: ReadinessState enum (6 states), WriterState enum (7 states), is_queryable() and should_defer() predicates
- Depends on: Storage layer (persists state in meta table)
- Used by: main.rs (state transitions), server tool handlers (gating queries)

**Cross-Process Lock:**
- Purpose: Serialize write-heavy index phases across concurrent qartez processes
- Location: src/lock.rs
- Contains: RepoLock wrapping fs4::FileExt advisory file lock, DEFAULT_ACQUIRE_DEADLINE (30s), PID-based lock file
- Depends on: fs4
- Used by: main.rs (background indexer), cli_runner.rs (CLI indexing), watch.rs (watcher)

**File Watcher:**
- Purpose: Incremental re-indexing of changed files while MCP server runs
- Location: src/watch.rs
- Contains: Watcher struct, WatchBatch (changed/deleted), debounce logic (500ms), DEFAULT_WRITER_CHUNK_SIZE (50)
- Depends on: notify, ignore, Storage layer, Index parser
- Used by: main.rs (attached per project root at startup)

**Modification Guard Engine:**
- Purpose: Core logic for qartez-guard -- evaluate PageRank and blast-radius thresholds
- Location: src/guard.rs
- Contains: GuardConfig, FileFacts, HookInput, touch_ack, ack file protocol (.qartez/acks/hash)
- Depends on: serde, Storage layer
- Used by: src/bin/guard.rs (hook binary), src/server/mod.rs (touch_ack)

**Toolchain Detection:**
- Purpose: Auto-detect build/test/lint/typecheck commands from known toolchains
- Location: src/toolchain.rs
- Contains: DetectedToolchain, detect_all_toolchains (Cargo, npm, Go, Python, Make, Gradle)
- Depends on: Filesystem probing (std only)
- Used by: src/server/tools/project.rs (qartez_project tool)

**IDE Setup Wizard (standalone binary):**
- Purpose: Detect installed IDEs, interactive checkbox prompt, configure MCP entries
- Location: src/bin/setup.rs
- Contains: Embedded hook snippets, IDE detection for 19 editors, update check, --uninstall mode
- Depends on: dialoguer, console, clap, chrono
- Used by: User-invoked as qartez-setup

**Modification Guard Hook (standalone binary):**
- Purpose: Claude Code PreToolUse hook -- reads stdin JSON, denies edit on load-bearing files without ack
- Location: src/bin/guard.rs
- Contains: CLI argument parsing, stdin JSON deserialization, DB query for FileFacts, deny/allow decision
- Depends on: guard.rs, graph::blast, storage::read, rusqlite
- Used by: Claude Code PreToolUse hook system

**Benchmark Harness (feature-gated):**
- Purpose: Comparative benchmark -- MCP tool vs non-MCP, token/latency/quality reports
- Location: src/benchmark/ (mod.rs, grounding.rs, judge.rs, profiles/, report.rs, scenarios.rs, set_compare.rs, sim_runner.rs, targets.rs, tokenize.rs)
- Contains: 28 scenarios, LatencyConfig, LLM-judge, per-language profiles (Rust, TS, Python, Go, Java)
- Depends on: tiktoken-rs, glob (benchmark feature)
- Used by: src/bin/benchmark.rs

**Opt-In Semantic Embedding (feature-gated):**
- Purpose: Local embedding model for natural-language code queries (hybrid FTS5 + vector)
- Location: src/embeddings.rs
- Contains: ORT (ONNX Runtime) + tokenizers (HuggingFace) integration
- Depends on: ort, tokenizers (semantic feature)
- Used by: src/server/tools/semantic.rs

## Data Flow

**Startup (MCP Server Path):**
1. Parse CLI args, resolve Config -- main.rs via cli.rs + config.rs
2. Open SQLite DB, readiness ColdStart -- storage/mod.rs, readiness.rs
3. Compute workspace fingerprint, compare with stored -- index/fingerprint.rs
4. If mismatch or --reindex: background index -> PageRank -> symbol PageRank -> co-change -> persist fingerprint -> WAL checkpoint
5. Set readiness to Ready -- readiness.rs
6. Attach file watchers -- watch.rs
7. Start MCP server on stdin/stdout -- server/mod.rs

**CLI Subcommand Path:**
1. Parse CLI args, resolve Config -- cli.rs + config.rs
2. Open SQLite DB -- storage/mod.rs
3. Acquire cross-process lock -- lock.rs
4. Full index + PageRank + symbol PageRank + co-change (synchronous)
5. Dispatch to tool handler, format output -- cli_runner.rs
6. Print to stdout

**MCP Tool Call Flow:**
1. rmcp receives JSON-RPC request on stdin -- server/mod.rs
2. Handler routes to matching tool_router -- server/tools/mod.rs (42 routers composed with +)
3. Per-tool handler reads from SQLite via storage::read or storage::models
4. Results serialized to CallToolResult (text + optional JSON)

**Incremental Re-indexing (Watcher):**
1. notify fires change events -- watch.rs
2. Events debounced (500ms) into WatchBatch -- watch.rs
3. Acquire short-deadline lock (30ms), re-parse changed files, update DB
4. Recompute PageRank incrementally -- graph/pagerank.rs
5. Lock released -- lock.rs

## Key Abstractions

**QartezServer:**
- Purpose: Central MCP server state (SQLite conn, roots, aliases, watchers, parse cache, tiers)
- Location: src/server/mod.rs
- Pattern: rmcp::ServerHandler with tool and prompt annotated methods; state in Arc<Mutex<Connection>> + Arc<RwLock<...>>

**Config:**
- Purpose: Resolved project configuration (roots, DB path, aliases, flags, git depth, chunk size)
- Location: src/config.rs
- Pattern: Builder from CLI args + auto-detection; marker-file root discovery

**ParserPool:**
- Purpose: Thread-safe tree-sitter parser pool, one per language, with parse cache
- Location: src/index/parser.rs
- Pattern: BTreeMap of 37 grammars pre-loaded; rayon parallel parse method

**ReadinessState / WriterState:**
- Purpose: Index lifecycle signals (queryable, write-in-progress)
- Location: src/readiness.rs
- Pattern: SQLite-persisted state machine; is_queryable() and should_defer() per Allium spec

**RepoLock:**
- Purpose: Cross-process advisory lock for write-heavy phases
- Location: src/lock.rs
- Pattern: OS flock via fs4 on qartez_dir/index.lock; PID diagnostic; RAII guard

**GuardConfig / FileFacts / HookInput:**
- Purpose: Modification guard decision (thresholds + ack protocol)
- Location: src/guard.rs
- Pattern: Pure evaluation with env-var overridable thresholds; ack via filesystem touch files

## Entry Points

**qartez (main MCP server / CLI):**
- Location: src/main.rs
- Triggers: MCP server (stdin piped), CLI subcommand (Command arg), help (TTY without args)
- Responsibilities: Bootstrap project, manage indexing lifecycle, start MCP server or dispatch CLI, fire-and-forget update check

**qartez-guard (modification guard hook):**
- Location: src/bin/guard.rs
- Triggers: Claude Code PreToolUse hook (stdin JSON-RPC)
- Responsibilities: Check PageRank/blast threshold, check ack file, return deny or allow

**qartez-setup (IDE setup wizard):**
- Location: src/bin/setup.rs
- Triggers: User-invoked (post-install or --update-check)
- Responsibilities: Detect 19 IDEs, interactive checkbox, configure MCP entries, install skill files

**benchmark (benchmark harness):**
- Location: src/bin/benchmark.rs
- Triggers: make bench / make bench-all
- Responsibilities: Run 28 scenarios, MCP vs non-MCP tokens/latency, reports to reports/

## Error Handling

**Strategy:** QartezError enum with thiserror (8 variants: Db, Io, Git, Parse, FileNotFound, SymbolNotFound, NoProjectRoot, Integrity). CLI via anyhow. Server-tool via rmcp::model::ErrorData. Guard fail-open (unexpected -> allow).

## Cross-Cutting Concerns

**Logging:** tracing with tracing-subscriber (env-filter, stderr). --log-level (default: info). Startup telemetry for DB and WAL sizes.

**Caching:** Tree-sitter parse cache (src/server/cache.rs). Watch debouncing (500ms). Workspace fingerprint short-circuits full re-index on startup.

**Storage:** .qartez/index.db (SQLite, WAL, incremental auto-vacuum, FTS5, busy_timeout=5000, cache_size=-64000). Cross-process writes via RepoLock advisory flock.

**Concurrency:** tokio for server and watcher. rayon for indexing and PageRank. SQLite via Arc<Mutex<Connection>>. WriterState tracks FullIndexing / IncrementalIndexing / Idle for readiness protocol.
