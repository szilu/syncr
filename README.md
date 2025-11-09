# SyncR - Fast Deduplicating Filesystem Synchronizer

[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A fast, efficient filesystem synchronizer written in Rust with content-determined chunking and data deduplication.

## ⚠️ Status

**Active Development** - Core functionality works, but this is an early-stage project. Use with caution on important data.

**Current Version**: 0.1.0 (Pre-release)

## Features

### ✅ Implemented

- **N-way synchronization** - Sync multiple directories simultaneously (2+ locations)
- **Content-determined chunking** - Uses Bup rolling hash algorithm for efficient deduplication
- **BLAKE3 hashing** - 6-20x faster than SHA1, cryptographically secure
- **Remote sync via SSH** - Sync across machines transparently
- **Smart caching** - redb-based metadata cache with mtime-based change detection
- **Path-level locking** - Prevents concurrent syncs on same directories
- **Stale lock detection** - Automatic recovery from crashed sync operations
- **Zero unsafe code** - 100% safe Rust (`#![forbid(unsafe_code)]`)
- **Comprehensive error handling** - No panics/unwraps in production code
- **Structured logging** - Configurable tracing via `RUST_LOG` environment variable
- **Small binary** - Static linking produces ~1.5MB MUSL binary
- **Protocol v2** - Stable text-based protocol with version handshake

### 🚧 Partially Implemented

- **Conflict detection** - Basic latest-mtime resolution works
- **Library API** - SyncBuilder pattern available but not fully wired up
- **Protocol v3** - Version negotiation implemented, commands not yet migrated

### ❌ Not Yet Implemented

- **Advanced conflict resolution** - Interactive resolution, three-way merge
- **Directory structure creation** - Needs improvement for nested directories
- **Include/exclude patterns** - Infrastructure exists but not integrated
- **Progress callbacks** - API designed but not wired up
- **Configuration files** - Currently CLI-only

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

```
syncr (parent)
├── Node 1 (local)  ──→ syncr serve ./dir1
├── Node 2 (local)  ──→ syncr serve ./dir2
├── Node 3 (remote) ──→ ssh remote1 syncr serve dir
└── Node 4 (remote) ──→ ssh remote2 syncr serve dir
```

**Protocol**: Text-based with JSON5 (v3) or colon-delimited (v2) commands over stdin/stdout

**Phases**:
1. LIST - Collect file metadata and chunk hashes
2. DIFF - Detect conflicts and changes
3. RESOLVE - Handle conflicts (auto or interactive)
4. WRITE - Transfer missing chunks and create file metadata
5. COMMIT - Rename temporary files to final locations

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

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture

# Run specific test
cargo test test_name

# Check for issues
cargo clippy
```

## Dependencies

**Core**:
- `blake3` - Fast cryptographic hashing
- `redb` - Embedded database for caching
- `json5` - Human-friendly serialization
- `tokio` - Async runtime
- `tracing` - Structured logging

**Chunking**:
- `rollsum` - Bup rolling hash algorithm

**Optional**:
- `ratatui` + `crossterm` - TUI support (feature: `tui`)

## Performance

- **Chunking**: BLAKE3 provides 6-20x speedup vs SHA1
- **Binary size**: ~1.5MB static MUSL build
- **Memory**: Efficient streaming, minimal memory footprint
- **Network**: Only changed chunks transferred

## Comparison with Other Tools

| Feature | SyncR | rsync | Unison | Syncthing | rclone |
|---------|-------|-------|--------|-----------|--------|
| **N-way sync** | ✅ | ❌ | ❌ (2-way) | ✅ | ❌ |
| **Content deduplication** | ✅ | ❌ | ❌ | ✅ | ❌ |
| **Bidirectional** | ✅ | ❌ | ✅ | ✅ | ✅ |
| **Binary compatibility** | ✅ Single binary | ✅ | ❌ OCaml issues | ✅ | ✅ |
| **Continuous sync** | ❌ | ❌ | ❌ | ✅ | ❌ |
| **Cloud support** | ❌ | ✅ (limited) | ❌ | ❌ | ✅ |
| **Conflict resolution** | 🚧 Basic | N/A | ✅ Advanced | ✅ | 🚧 |
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

## Documentation

- **User Documentation**: See `CLAUDE.md` for build commands and usage
- **Developer Documentation**: See `claude-docs/` directory
  - `STATUS.md` - Current implementation status
  - `PROTOCOL-SPECIFICATION.md` - Protocol details
  - `CODEBASE-STRUCTURE.md` - Code organization
  - `ERROR_HANDLING_SUMMARY.md` - Safety analysis

## Contributing

This is a learning project and first Rust project by the author. Contributions welcome!

**Areas needing work**:
- Directory structure creation improvements
- Interactive conflict resolution UI
- Configuration file support
- Include/exclude patterns
- Progress reporting via callbacks
- Protocol v3 completion (JSON5 + binary chunks)

## Safety & Quality

- ✅ **Zero unsafe blocks** - `#![forbid(unsafe_code)]`
- ✅ **No panics/unwraps** - Comprehensive error handling
- ✅ **60+ tests** - Library and integration tests
- ✅ **Clean clippy** - No warnings
- ✅ **Crash recovery** - Stale lock detection, temp file cleanup

## License

MIT License - See LICENSE file for details

## Author

Created as a Rust learning project.

## Warning

⚠️ **USE AT YOUR OWN RISK** ⚠️

This is work-in-progress software. While core functionality works and safety measures are in place:
- Always test on non-critical data first
- Keep backups of important files
- Some features are incomplete (see Status section)
- The author is not responsible for data loss

**Production Readiness**: Not recommended for production use yet. Suitable for personal use and testing.
