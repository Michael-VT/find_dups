# find_dups: Cross-Language Duplicate File Finder

A comprehensive duplicate file finder implemented in Go, Python, and Rust with identical algorithms for performance comparison and production use.

A comprehensive duplicate file finder implemented in Go, Python, Rust, JavaScript, and C++ with identical algorithms for performance comparison and production use.
## Overview

`find_dups` scans one or more directories recursively, identifies duplicate files using SHA-256 hashing, and generates reports and deletion scripts. It uses parallel processing to efficiently handle large file collections.

### Key Features

- **Multi-language implementation**: Go, Python, Rust, JavaScript, and C++ versions for performance comparison
- **Parallel hashing**: Utilizes all CPU cores for fast duplicate detection
- **Safety**: Generates a deletion script rather than deleting files directly
- **Detailed reports**: CSV exports with file metadata and timestamps
- **Cross-drive support**: Can scan multiple directories across different mount points

## Algorithm

All five implementations follow the same algorithm:

1. **Collect files** — Recursive walk through all specified directories, recording path, size, birth time, and modification time
2. **Group by size** — Only files sharing a size with at least one other file proceed to hashing (optimization)
   - Go: goroutines with channel-based worker pool
   - Python: `multiprocessing.Pool`
   - Rust: `rayon` parallel iterator
   - JavaScript: `worker_threads` with Worker pool
   - C++: `std::thread` with thread pool
5. **Generate outputs**:
   - `duplicates_<lang>.csv` — All duplicate file groups with full metadata
   - `sort_dup_<lang>.csv` — All files sorted by size (descending)
   - `duprm_<lang>.sh` — Bash script that deletes all duplicates except the first (by ID) in each group

## Output Files

### duplicates_<lang>.csv
CSV file containing all duplicate files grouped by content. Columns:
- `FileID`: Sequential file identifier
- `Path`: Full file path
- `Size`: File size in bytes
- `Hash`: SHA-256 hash (hexadecimal)
- `CreationTime`: File creation timestamp (ISO 8601)
- `ModificationTime`: File modification timestamp (ISO 8601)

### sort_dup_<lang>.csv
CSV file containing all scanned files sorted by size (descending). Same columns as `duplicates_<lang>.csv`.

### duprm_<lang>.sh
Executable bash script that removes duplicate files, preserving the first file (lowest FileID) in each duplicate group. **Review this script before executing** to ensure you don't delete important files.

## Installation & Usage

### Go Implementation

**Prerequisites**: Go 1.16+

**Build**:
```bash
cd find_dups_go
go build -o find_dups_go find_dups_go.go
```

**Run**:
```bash
./find_dups_go /path/to/scan1 /path/to/scan2 ...
```

**Dependencies**: Standard library only

### Python Implementation

**Prerequisites**: Python 3.8+

**Run**:
```bash
cd find_dups_pthon
python3 find_dups.py /path/to/scan1 /path/to/scan2 ...
```

**Dependencies**: Standard library only

### Rust Implementation

**Prerequisites**: Rust 1.70+, Cargo

**Build**:
```bash
cd find_dups_rust
cargo build --release
```

**Run**:
```bash
./target/release/find_dups /path/to/scan1 /path/to/scan2 ...
```

**Dependencies** (see `Cargo.toml`):
- `walkdir` 2.5 — Directory traversal
- `sha2` 0.10 — SHA-256 hashing
- `csv` 1.4 — CSV writing
- `chrono` 0.4 — Time formatting
- `rayon` 1.12 — Parallel processing

### JavaScript (Node.js) Implementation

**Prerequisites**: Node.js 16+ (with worker_threads support)

**Run**:
```bash
cd find_dups_js
node find_dups.js /path/to/scan1 /path/to/scan2 ...
```

**Dependencies**: Standard library only (`crypto`, `fs`, `worker_threads`)

### C++ Implementation

**Prerequisites**: g++ with C++17 support, OpenSSL (libcrypto)

**Build**:
```bash
cd find_dups_cp
g++ -std=c++17 -O3 -pthread -I/usr/local/opt/openssl/include -L/usr/local/opt/openssl/lib find_dups.cpp -o find_dups_cpp -lcrypto -Wno-deprecated-declarations
```

**Run**:
```bash
./find_dups_cpp /path/to/scan1 /path/to/scan2 ...
```

**Dependencies**:
- OpenSSL (libcrypto) — SHA-256 hashing
- Standard library only for filesystem and threading

## Benchmark Results

Tested on approximately 149,000 files across two directories (local SSD + external USB drive):

| Metric                | Python     | Rust       | Go         | JavaScript | C++         |
|-----------------------|------------|------------|------------|------------|-------------|
| Files scanned         | 148,819    | 148,819    | 148,819    | 148,819    | 148,819     |
| Duplicates found      | 696        | 696        | 696        | 696        | 696         |
| Total time            | ~3:17      | ~3:20      | ~3:12      | ~3:40      | ~3:04       |
| Output suffix         | _py        | _rs        | _go        | _js        | _cpp        |

**Notes**:
- Times in `minutes:seconds` format
- All implementations now produce identical results: 148,819 files scanned, 696 duplicates found
- All implementations ignore symbolic links and process only regular files
- C++ shows the best performance, followed by Go, Python, Rust, and JavaScript

## Evaluation & Recommendations

### Strengths

- **Practical value**: High — solves a real problem of finding duplicates across multiple directories and drives
- **Safety**: Good — generates a deletion script for review rather than deleting directly
- **Performance**: All three implementations use parallel processing effectively
- **Transparency**: CSV reports allow for detailed analysis before deletion

### Known Issues

1. **Platform limitations**:
   - Go uses macOS-specific `syscall.Stat_t` for birth time
   - Rust uses `std::os::darwin::fs::MetadataExt` for birth time
   - C++ uses `statfs` for macOS birth time
   - JavaScript and Python use platform-independent approaches
   - All require conditional compilation or adaptation for Linux/Windows support
### Which Implementation to Use?

- **For production use on macOS**: C++ — fastest overall performance (~3:04)
- **For production use on macOS (alternative)**: Go — single binary with no dependencies, close second (~3:12)
- **For cross-platform development**: Rust — easiest to adapt with `#[cfg(target_os)]` attributes
- **For quick scripting/prototyping**: Python — easiest to modify
- **For Node.js environments**: JavaScript — integrates well with JS/TS tooling
- **For maximum performance**: C++ — best performance but requires compilation and OpenSSL
1. **Progress bar** — Add real-time progress indication during hashing phase
2. **Cross-platform birth time** — Use conditional compilation for Linux/Windows
3. **Partial hashing optimization** — Hash first/last N KB + size before full file hash
4. **Configurable output** — Allow specifying output directory and file prefixes
5. **Interactive mode** — Simple TUI for reviewing duplicates before deletion
6. **Dry-run mode** — Show what would be deleted without generating a script
7. **Move instead of delete** — Option to move duplicates to a staging directory
8. **Minimum size filter** — Skip files below a configurable threshold (e.g., <1KB)
9. **Symlink/hardlink deduplication** — Replace duplicates with hardlinks to save space without deleting

## License

This project is provided as-is for educational and practical use.

## Contributing

Contributions are welcome, especially for:
- Adding Windows/Linux compatibility
- Implementing any of the future enhancements listed above
