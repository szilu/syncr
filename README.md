# SyncR - Fast Deduplicating Filesystem Synchronizer

[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A fast, efficient n-way filesystem synchronizer written in Rust with content-determined chunking and data deduplication. Sync multiple directories simultaneously across local and remote machines.

## ⚠️ Status

**Active Development** - Core functionality is working and tested with 8,300+ lines of comprehensive tests. This is a learning project and the author's first Rust project. Use with caution on important data.

**Current Version**: 0.3.0 (Alpha)

## Features

### ✅ Fully Implemented

**Core Sync Engine:**
- **N-way synchronization** - Sync 2+ directories simultaneously (local and/or remote)
- **Content-determined chunking** - Bup rolling hash algorithm for efficient deduplication
- **BLAKE3 hashing** - 6-20x faster than SHA1, cryptographically secure
- **Remote sync via SSH** - Transparent multi-node synchronization
- **Smart caching** - redb-based metadata cache with mtime-based change detection
- **Atomic operations** - Temporary file strategy with commit phase

**Reliability & Safety:**
- **Zero unsafe code** - 100% safe Rust (`#![deny(unsafe_code)]`)
- **Comprehensive error handling** - No panics/unwraps in production code
- **Error resilience system** - Per-file error handling without aborting entire sync
- **Path-level locking** - Prevents concurrent syncs on same directories
- **Stale lock detection** - Automatic recovery from crashed sync operations
- **Signal handling** - Graceful shutdown on SIGINT/SIGTERM with cleanup

**Protocol & Architecture:**
- **Protocol v3** - JSON5-based protocol with structured error messages (ONLY supported version)
- **Multi-version negotiation** - Automatic protocol version selection
- **Node capabilities** - Per-node metadata capability detection
- **Modular architecture** - Clean separation: protocol layer, sync logic, file operations

**Configuration & Control:**
- **Unified config system** - Single Config struct (eliminates 27 fragmented config types)
- **Conflict resolution strategies** - 6 modes: PreferFirst, PreferLast, PreferNewest, PreferOldest, PreferLargest, Interactive
- **Delete protection** - Configurable safety limits (max count, max percentage, backup mode)
- **Delete modes** - Sync, NoDelete, DeleteAfter, DeleteExcluded, Trash
- **Metadata strategies** - Strict, Smart, Relaxed, ContentOnly
- **Symlink modes** - Preserve, Follow, Ignore, Relative

**Filtering & Exclusion:**
- **Exclusion patterns** - Glob-based file/directory exclusion
- **Inclusion patterns** - Override exclusions for specific patterns
- **Gitignore support** - Honor .gitignore, .syncignore files
- **Custom ignore files** - Configure additional ignore file patterns
- **File property filters** - Size, age, type filters

**Developer Features:**
- **Structured logging** - Full tracing support via `RUST_LOG`
- **Library API** - Use SyncR as a Rust library (see `src/lib.rs`)
- **Extensive testing** - 19 test files, 8,300+ lines of tests
- **Validation module** - Centralized path, config, and cache validation

### 🚧 Partially Implemented

- **Progress callbacks** - API exists but not fully integrated with all sync operations
- **Config file loading** - Config struct complete, file discovery/parsing needs work
- **TUI mode** - Terminal UI framework exists (feature-gated) but needs updating

### ❌ Not Yet Implemented

- **Interactive conflict resolution** - UI for manual conflict resolution
- **Configuration files** - Config struct ready, but TOML/JSON loading incomplete
- **Continuous/watch mode** - File system watching for automatic sync
- **Archive metadata** - Storing sync state in archive files

## Quick Start

### Installation

```bash
git clone https://github.com/szilu/syncr.git
cd syncr
cargo build --release

# Optional: Install to system
sudo cp target/release/syncr /usr/local/bin/
```

### Basic Usage

```bash
# Sync two local directories
syncr sync ./dir1 ./dir2

# Sync with remote directories (requires syncr on remote PATH)
syncr sync ./local remote1:dir remote2.example.com:dir

# Sync with progress display
syncr sync ./dir1 ./dir2 --progress

# Sync quietly (auto-skip conflicts)
syncr sync ./dir1 ./dir2 --quiet

# Inspect directory structure and chunks
syncr dump ./directory
```

### Environment Variables

```bash
# Enable logging
RUST_LOG=info syncr sync ./dir1 ./dir2

# Debug logging
RUST_LOG=debug syncr sync ./dir1 ./dir2

# Module-specific logging
RUST_LOG=syncr::serve=trace syncr sync ./dir1 ./dir2
```

## How It Works

1. **Content-Determined Chunking**: Files are split into variable-sized chunks using rolling hash
2. **Deduplication**: Chunks are hashed with BLAKE3, identical chunks are only transferred once
3. **Multi-Node Protocol**: Parent process coordinates, child processes run on each location
4. **Smart Caching**: File metadata cached in redb database to avoid re-scanning unchanged files
5. **Atomic Operations**: Temporary files used, renamed only on successful sync

## Architecture

### Multi-Node Parent-Child Model

```
syncr (parent process)
├── Node 1 (local)  ──→ syncr serve ./dir1
├── Node 2 (local)  ──→ syncr serve ./dir2
├── Node 3 (remote) ──→ ssh remote1 syncr serve dir
└── Node 4 (remote) ──→ ssh remote2 syncr serve dir
```

The parent process orchestrates sync across all nodes. Each node runs a child process in "serve mode" that responds to protocol commands.

### Protocol v3 (JSON5)

**Current implementation uses Protocol v3 exclusively** (no backward compatibility with v2).

**Transport**: JSON5-formatted commands over stdin/stdout
**Features**:
- Structured error messages with severity/code/recovery actions
- Per-file error reporting without aborting sync
- Node capability negotiation
- Binary chunk transfer (base64-encoded in JSON5)

**Protocol Commands**:
- `VER` - Version negotiation handshake
- `CAP` - Capability exchange (metadata support, features)
- `LIST` - Collect directory tree, file metadata, chunk hashes
- `WRITE` - Send file metadata and chunks to create/update files
- `READ` - Request specific chunks from node
- `COMMIT` - Atomically rename temp files to final locations
- `QUIT` - Clean shutdown

### Sync Pipeline (8 Phases)

1. **Connect** - Spawn child processes, negotiate protocol version
2. **Capability Exchange** - Detect per-node metadata capabilities
3. **Collection** - Each node lists files and chunks
4. **Diff** - Compare files across nodes, detect conflicts
5. **Conflict Resolution** - Auto-resolve or ask user (based on config)
6. **Metadata Write** - Send file/directory creation commands
7. **Chunk Transfer** - Transfer missing chunks between nodes
8. **Commit** - Atomic rename of temporary files

## Building

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release

# With TUI support
cargo build --release --features tui

# Static MUSL build (Linux only)
cargo build --release --target x86_64-unknown-linux-musl
```

## Testing

SyncR has comprehensive test coverage with **19 test files** and **8,300+ lines of tests**.

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture

# Run specific test
cargo test test_name

# Run tests with timing
cargo test -- --test-threads=1 --nocapture

# Check for issues (should be clean)
cargo clippy

# Format code
cargo fmt
```

### Test Categories

**Integration Tests** (`tests/`):
- `integration_test.rs` - End-to-end sync scenarios
- `real_file_integration_test.rs` - Real filesystem operations
- `library_api_test.rs` - Public API testing
- `error_handling_test.rs` - Error scenario coverage
- `error_resilience.rs` - Per-file error handling
- `protocol_list_test.rs` - Protocol LIST command
- `protocol_scenarios_test.rs` - Protocol edge cases
- `protocol_error_handling_test.rs` - Protocol error paths
- `conflict_resolution_test.rs` - Conflict handling
- `config_loading_test.rs` - Configuration system
- `config_options_test.rs` - Config option parsing
- `capability_metadata_test.rs` - Metadata capabilities
- `capability_fallback_test.rs` - Capability degradation
- `heterogeneous_sync_test.rs` - Mixed node capabilities
- `connection_error_test.rs` - Connection failures
- `signal_handling_test.rs` - SIGINT/SIGTERM handling
- `deadlock_detection_test.rs` - Concurrency safety
- `precommit_verification_test.rs` - Pre-commit checks
- `chunking_test.rs` - Content-determined chunking

## Dependencies

**Core**:
- `blake3` - Fast cryptographic hashing (6-20x faster than SHA1)
- `redb` - Embedded database for metadata caching
- `json5` - Protocol serialization with human-friendly format
- `serde_json5` - JSON5 parsing for error metadata
- `tokio` - Async runtime for I/O operations
- `tracing` + `tracing-subscriber` - Structured logging
- `clap` - CLI argument parsing

**Chunking & Hashing**:
- `rollsum` - Bup rolling hash algorithm for content-determined chunking
- `base64` - Chunk encoding in protocol

**Filesystem Operations**:
- `ignore` - .gitignore parsing
- `globset` - Pattern matching for exclusions
- `uuid` - Unique identifiers for locks and sessions
- `sysinfo` - Process detection for stale locks

**Optional**:
- `ratatui` + `crossterm` - TUI support (feature: `tui`)

**Development**:
- `tempfile` - Temporary directories for tests
- `filetime` - Timestamp manipulation in tests

## Performance

- **Hashing**: BLAKE3 provides 6-20x speedup vs SHA1, cryptographically secure
- **Binary size**: ~6.2MB release build, ~1.5MB static MUSL build (with stripping)
- **Memory**: Efficient streaming with redb caching, minimal memory footprint
- **Network**: Only changed chunks transferred, deduplication across all nodes
- **Caching**: Smart mtime-based change detection avoids re-scanning unchanged files
- **Concurrency**: Async I/O with tokio for efficient multi-node operations

### Code Documentation
```bash
# Generate and open rustdoc
cargo doc --open

# Generate with private items
cargo doc --document-private-items --open
```

## Comparison with Other Tools

| Feature | SyncR | rsync | Unison | Syncthing | rclone |
|---------|-------|-------|--------|-----------|--------|
| **N-way sync** | ✅ | ❌ | ❌ (2-way) | ✅ | ❌ |
| **Content deduplication** | ✅ | ❌ | ❌ | ✅ | ❌ |
| **Bidirectional** | ✅ | ❌ | ✅ | ✅ | ✅ |
| **Binary compatibility** | ✅ Single binary | ✅ | ❌ OCaml issues | ✅ | ✅ |
| **Continuous sync** | ❌ | ❌ | ❌ | ✅ | ❌ |
| **Cloud support** | ❌ | ✅ (limited) | ❌ | ❌ | ✅ |
| **Conflict resolution** | ✅ 6 strategies | N/A | ✅ Advanced | ✅ | 🚧 |
| **Maturity** | 🚧 Alpha | ✅ Stable | ✅ Stable | ✅ Stable | ✅ Stable |

**When to use SyncR:**
- Need to sync 3+ directories simultaneously
- Large files with frequent partial changes (benefits from chunking)
- Want modern hashing (BLAKE3) and deduplication
- Prefer a single lightweight binary
- Rust-based infrastructure

**When to use alternatives:**
- **rsync**: One-way sync, mature and battle-tested
- **Unison**: Two-way sync with advanced conflict resolution, mature
- **Syncthing**: Continuous background sync, easy GUI setup
- **rclone**: Cloud storage sync (S3, Google Drive, etc.)

**Note**: SyncR is in early development. Other tools are more mature and production-ready.

## Contributing

This is a learning project and first Rust project by the author. Contributions welcome!

**Priority Areas for Contribution**:
1. **Config file loading** - TOML/JSON parsing and file discovery (Config struct ready)
2. **Interactive conflict resolution** - Terminal UI for manual conflict resolution
3. **TUI mode updates** - Update existing TUI to work with current sync API
4. **Progress callback integration** - Wire up existing progress API to all sync phases
5. **Protocol optimization** - Reduce code duplication in v3_server.rs (see STATUS.md)
6. **Watch mode** - File system watching for continuous sync
7. **Documentation** - More examples, tutorials, architecture docs

**Code Quality Tasks**:
- Metadata type consolidation (FileData vs FileSystemEntry)
- Protocol layer cleanup (see claude-docs/STATUS.md Phase 1B)
- Additional test coverage for edge cases

See `claude-docs/` directory for detailed implementation plans and current status.

## Safety & Quality

- ✅ **Zero unsafe blocks** - `#![deny(unsafe_code)]` enforced
- ✅ **No panics/unwraps** - Comprehensive error handling with Result types
- ✅ **19 test files, 8,300+ lines** - Extensive integration and unit tests
- ✅ **Clean clippy** - Only 2 minor warnings (unused imports)
- ✅ **Crash recovery** - Stale lock detection, temp file cleanup, signal handling
- ✅ **Error resilience** - Per-file error handling without aborting sync
- ✅ **Comprehensive logging** - Structured tracing with context
- ✅ **Atomic operations** - Temporary file strategy with commit phase

## License

MIT License - See LICENSE file for details

## Author

Created as a Rust learning project.

## Warning

⚠️ **USE AT YOUR OWN RISK** ⚠️

This is alpha software and a learning project. While core functionality works and extensive safety measures are in place:
- ✅ Core sync engine is working and well-tested (8,300+ lines of tests)
- ✅ Error handling prevents data corruption
- ✅ Atomic operations and crash recovery implemented
- ⚠️ Some features incomplete (config file loading, interactive conflicts)
- ⚠️ Limited real-world testing - this is the author's first Rust project
- ⚠️ Protocol may change in future versions

**Recommendations**:
1. Always test on non-critical data first
2. Keep backups of important files
3. Start with simple 2-way sync before n-way
4. Review logs with `RUST_LOG=info` for first few syncs
5. Use `--dry-run` when available (future feature)

**Production Readiness**: Not recommended for production use yet. Suitable for personal use, testing, and learning.

---

## Project Status Summary

**What Works Well:**
- ✅ N-way synchronization (tested with 2-4 nodes)
- ✅ Content-determined chunking and deduplication
- ✅ Remote sync via SSH
- ✅ Error resilience (per-file error handling)
- ✅ Protocol v3 implementation
- ✅ Comprehensive test coverage

**What Needs Work:**
- 🚧 Config file loading (struct ready, parsing incomplete)
- 🚧 Interactive conflict resolution (auto-resolution works)
- 🚧 Progress callbacks (API exists, not fully wired)
- 🚧 TUI mode (needs updating for current API)

**Known Limitations:**
- Protocol v3 only (no backward compatibility)
- Config via CLI flags only (no config files yet)
- Conflicts resolved by rules, not interactively
- No continuous/watch mode
- Limited to SSH for remote connections

**Version History:**
- v0.3.0 (current) - Protocol v3, unified config, error resilience, extensive testing
- v0.2.0 - Protocol v2, basic conflict resolution
- v0.1.0 - Initial implementation

See `claude-docs/STATUS.md` for detailed implementation status.
