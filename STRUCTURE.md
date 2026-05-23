# Codebase Structure

## Directory Layout

```
qartez-mcp/
├── .allium/                 # Behavioral specification (Allium contract)
├── .beads/                  # Beads issue tracker runtime state
├── .github/                 # CI workflows, issue templates, Dependabot
├── .qartez/                 # Qartez's own index database + ack files
├── benchmarks/              # Pinned OSS fixture configs for multi-language bench
├── docs/                    # Architecture, configuration, plans, agent docs
├── fuzz/                    # Cargo-fuzz targets for parser security
├── reports/                 # Generated benchmark artifacts (markdown + JSON)
├── scripts/                 # Install hooks, IDE snippets, skill references, plugin
├── src/                     # All Rust source code
│   ├── bin/                 # Standalone binaries (setup, guard, benchmark)
│   ├── benchmark/           # Benchmark harness (feature-gated behind benchmark)
│   ├── git/                 # Git history mining (co-change, diff, trend, knowledge)
│   ├── graph/               # Graph analysis (pagerank, blast, leiden, boundaries, security, wiki)
│   ├── index/               # Core indexing engine (parser, walker, symbols, languages)
│   │   └── languages/       # 37 language adapters with tree-sitter grammars
│   ├── server/              # MCP server handler + tool dispatch
│   │   └── tools/           # 42 per-tool handler modules
│   └── storage/             # SQLite persistence (schema, read, write, models, maintenance)
├── tests/                   # Integration and regression tests (~45 test files)
├── Cargo.toml               # Package manifest (4 binaries, 95 dependencies, 3 features)
├── Makefile                 # CI / release / bench targets
├── install.sh               # POSIX installer
├── install.ps1              # Windows installer
├── rust-toolchain.toml      # MSRV 1.88
├── deny.toml                # cargo-deny advisory/license configuration
└── osv-scanner.toml         # OSV vulnerability scanner configuration
```

## Directory Purposes

**src/:**
- Purpose: All Rust source code for the project
- Contains: Library root (lib.rs), entry point (main.rs), and 14 module directories
- Key files: src/lib.rs, src/main.rs

**src/bin/:**
- Purpose: Standalone binary entry points, each producing a separate executable
- Contains: setup.rs (IDE setup wizard), guard.rs (modification guard hook), benchmark.rs (benchmark harness)
- Key files: src/bin/setup.rs (19-IDE auto-detection, ~4,442 lines)

**src/index/:**
- Purpose: Core indexing engine -- parses source files, extracts symbols/imports/references, computes structural shape hashes
- Contains: mod.rs (full + incremental index orchestration, ~4,211 lines), parser.rs (tree-sitter parser pool), parser_workers.rs (parallel file-level parsing), walker.rs (file discovery), symbols.rs (symbol/import/reference extraction + shape hashing), fingerprint.rs (workspace fingerprint)
- Key files: src/index/mod.rs, src/index/symbols.rs

**src/index/languages/:**
- Purpose: 37 per-language tree-sitter grammar adapters implementing symbol/import/complexity extraction
- Contains: One file per language -- rust_lang.rs, typescript.rs, python.rs, go.rs, java.rs, c_lang.rs, cpp.rs, etc.
- Key files: src/index/languages/mod.rs (language registration + complexity dispatch)

**src/graph/:**
- Purpose: Graph analysis algorithms running on the import graph
- Contains: pagerank.rs (PageRank + symbol PageRank), blast.rs (transitive impact BFS), leiden.rs (community detection), boundaries.rs (architecture rule engine), security.rs (pattern-based security scanner), wiki.rs (architecture wiki renderer)
- Key files: src/graph/pagerank.rs, src/graph/blast.rs

**src/git/:**
- Purpose: Git history mining -- co-change pairs, diff impact, complexity trends, bus-factor
- Contains: cochange.rs (pair mining from commit history), diff.rs (git diff range impact), trend.rs (per-function complexity over time), knowledge.rs (blame-based authorship concentration)
- Key files: src/git/cochange.rs

**src/server/:**
- Purpose: MCP protocol server -- JSON-RPC dispatch to tool handlers
- Contains: mod.rs (QartezServer struct + server handler, ~1,079 lines), prompts.rs (5 workflow prompt templates), tiers.rs (progressive tool disclosure), cache.rs (tree-sitter parse cache), helpers.rs (shared utilities), params.rs (tool parameter structs), overview.rs (map generation), treesitter.rs (integration helpers), mcp_instructions.md
- Key files: src/server/mod.rs, src/server/tools/mod.rs

**src/server/tools/:**
- Purpose: One file per MCP tool, each contributing a tool_router impl block
- Contains: 42 handler modules -- map, find, read, grep, outline, impact, deps, stats, refs, calls, rename, move, rename_file, replace, insert, safe_delete, cochange, context, diff_impact, hotspots, clones, smells, health, refactor_plan, test_gaps, unused, boundaries, hierarchy, trend, security, knowledge, workspace, maintenance, wiki, tools_meta, semantic, project, refactor_common, understand
- Key files: src/server/tools/mod.rs (router composition with 42 + operators)

**src/storage/:**
- Purpose: SQLite persistence -- schema, read/write operations, maintenance, row structs
- Contains: schema.rs (table + index + FTS5), read.rs (query helpers), write.rs (mutation helpers + unchecked transactions), models.rs (row structs), maintenance.rs (vacuum, checkpoint, FTS optimize, startup telemetry)
- Key files: src/storage/schema.rs, src/storage/read.rs, src/storage/write.rs

**src/benchmark/:**
- Purpose: Comparative benchmark harness (feature-gated)
- Contains: profiles/ (per-language profiles -- rust, typescript, go, python, java), scenarios.rs (28 benchmark scenarios), judge.rs (LLM judge harness), report.rs (markdown/JSON writers), sim_runner.rs (non-MCP workflow simulation), tokenize.rs (cl100k_base token accounting)
- Key files: src/benchmark/scenarios.rs

**tests/:**
- Purpose: Integration and regression tests against the actual SQLite-backed index
- Contains: tools.rs (tool integration tests), cli_integration.rs, add_root.rs, cross_process_lock.rs, business_logic.rs, plus ~35 fp_regression_* test files
- Key files: tests/tools.rs

**benchmarks/:**
- Purpose: Pinned OSS fixture definitions for multi-language benchmarking
- Contains: README.md, fixtures.toml (pinned repos and tag references)

**scripts/:**
- Purpose: IDE support assets embedded by qartez-setup
- Contains: AGENTS.md.snippet, CLAUDE.md.snippet, GEMINI.md.snippet, cursor-rule.mdc, instructions.md, opencode-plugin.ts, skill/ (Claude Code skill with 7 doctrine references)
- Key files: scripts/skill/SKILL.md

## Key File Locations

**Entry Points:**
- src/main.rs: Main MCP server / CLI binary (qartez)
- src/bin/setup.rs: Interactive IDE setup wizard (qartez-setup)
- src/bin/guard.rs: Claude Code PreToolUse modification guard (qartez-guard)
- src/bin/benchmark.rs: Benchmark harness (benchmark, requires --features benchmark)

**Configuration:**
- Cargo.toml: Package manifest, 4 binaries, 3 features (benchmark, semantic, default), ~95 dependencies including 37 tree-sitter grammars
- src/config.rs: Project root detection, DB path resolution, CLI arg merge
- rust-toolchain.toml: MSRV 1.88
- deny.toml: cargo-deny advisory/license configuration
- osv-scanner.toml: OSV vulnerability scanner

**Core Logic:**
- src/index/mod.rs: Full and incremental indexing orchestration (~4,211 lines)
- src/index/symbols.rs: Symbol/import/reference extraction + AST shape hashing
- src/server/mod.rs: MCP server handler + QartezServer struct (~1,079 lines)
- src/server/tools/mod.rs: Tool router composition (42 tools)
- src/graph/pagerank.rs: PageRank on file-level and symbol-level import graphs
- src/graph/blast.rs: Transitive blast radius computation
- src/git/cochange.rs: Git co-change pair mining
- src/guard.rs: Modification guard evaluation engine
- src/watch.rs: File watcher with debouncing and chunked writes
- src/toolchain.rs: Build/test/lint command detection
- src/readiness.rs: Index lifecycle state machine
- src/lock.rs: Cross-process advisory file lock
**Tests:**
- tests/tools.rs: Integration tests for all MCP tools
- tests/cli_integration.rs: CLI subcommand integration tests
- tests/cross_process_lock.rs: Lock contention tests
- tests/fp_regression_*.rs: Feature-specific regression tests (~35 files)

## Naming Conventions

**Files:** Rust source uses snake_case.rs (e.g., cli_runner.rs, pagerank.rs, parser_workers.rs). Language adapters in src/index/languages/ use the language name suffixed with _lang to avoid Rust keyword clashes (rust_lang.rs, toml_lang.rs, c_lang.rs).

**Directories:** snake_case/ (e.g., src/server/tools/, src/index/languages/, src/git/). Feature-gated top-level modules follow the same convention but are only compiled when the corresponding cargo feature is enabled.

**Functions:** snake_case -- compute_pagerank, analyze_cochanges, full_index_multi, walk_directory.

**Types:** PascalCase -- QartezServer, GuardConfig, ParserPool, ReadinessState, RepoLock, ChangeSet, SymbolInsert, DetectedToolchain.

**Constants:** SCREAMING_SNAKE_CASE -- DEFAULT_PAGERANK_MIN, DEFAULT_ACK_TTL_SECS, DEFAULT_WRITER_CHUNK_SIZE, PROJECT_MARKERS, QARTEZIGNORE_FILENAME.

**Binaries:** The crate produces 4 binaries -- qartez, qartez-guard, qartez-setup, benchmark (feature-gated).

## Where to Add New Code

**New MCP tool:** src/server/tools/<tool-name>.rs -- create handler module with tool_router impl, then register in src/server/tools/mod.rs tool router composition (one + Self::qartez_<name>_router() line). Add CLI subcommand variant in src/cli.rs and dispatch in src/cli_runner.rs if needed.

**New language adapter:** src/index/languages/<lang-name>.rs -- implement the LanguageAdapter trait for symbol extraction, import resolution, and cyclomatic complexity. Register in src/index/languages/mod.rs. Add the tree-sitter grammar crate to Cargo.toml.

**New graph algorithm:** src/graph/<name>.rs -- implement the algorithm reading from storage models. Expose a public function taking &Connection + config. Wire into post-index pipeline in main.rs and cli_runner.rs if it should run during indexing.

**New git analysis:** src/git/<name>.rs -- implement analysis reading from git2 + storage. Register in src/git/mod.rs.

**New storage schema or query:** src/storage/schema.rs (schema migration), src/storage/read.rs (query), src/storage/write.rs (mutation). Add row structs to src/storage/models.rs.

**New binary entry point:** src/bin/<name>.rs -- implement CLI. Register [[bin]] section in Cargo.toml.

**New integration test:** tests/<name>.rs -- use tempfile for temporary databases, call storage::open_in_memory() or storage::open_db().

**Shared utilities:** src/str_utils.rs (string helpers), src/test_paths.rs (test path helpers), src/error.rs (error types).
