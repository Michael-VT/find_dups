# find_dups: Cross-Language Duplicate File Finder

A comprehensive duplicate file finder implemented in Go, Python, and Rust with identical algorithms for performance comparison and production use.

## Overview

`find_dups` scans one or more directories recursively, identifies duplicate files using SHA-256 hashing, and generates reports and deletion scripts. It uses parallel processing to efficiently handle large file collections.

### Key Features

- **Multi-language implementation**: Go, Python, and Rust versions for performance comparison
- **Parallel hashing**: Utilizes all CPU cores for fast duplicate detection
- **Safety**: Generates a deletion script rather than deleting files directly
- **Detailed reports**: CSV exports with file metadata and timestamps
- **Cross-drive support**: Can scan multiple directories across different mount points

## Algorithm

All three implementations follow the same algorithm:

1. **Collect files** — Recursive walk through all specified directories, recording path, size, birth time, and modification time
2. **Group by size** — Only files sharing a size with at least one other file proceed to hashing (optimization)
3. **Parallel SHA-256 hashing** — Compute cryptographic hashes in parallel:
   - Go: goroutines with channel-based worker pool
   - Python: `multiprocessing.Pool`
   - Rust: `rayon` parallel iterator
4. **Identify duplicates** — Group files by hash within size groups; all files in a hash group with >1 member are duplicates
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
python3 find_dups_python.py /path/to/scan1 /path/to/scan2 ...
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
./target/release/find_dups_rust /path/to/scan1 /path/to/scan2 ...
```

**Dependencies** (see `Cargo.toml`):
- `walkdir` 2.5 — Directory traversal
- `sha2` 0.10 — SHA-256 hashing
- `csv` 1.4 — CSV writing
- `chrono` 0.4 — Time formatting
- `rayon` 1.12 — Parallel processing

## Benchmark Results

Tested on approximately 149,000 files across two directories (local SSD + external USB drive):

| Metric                | Python     | Rust       | Go         |
|-----------------------|------------|------------|------------|
| Files scanned         | 149,044    | 148,819    | 148,819    |
| Files hashed          | 128,964    | 128,738    | 128,738    |
| Hashing time          | 1:55.751   | 2:07.520   | 1:31.992   |
| Total time            | 5:26.243   | 3:55.664   | 3:06.040   |
| Duplicates found      | 0          | 696        | 696        |
| Workers/threads       | 12         | 12         | 12         |

**Notes**:
- Times in `minutes:seconds.milliseconds` format
- The Python implementation found 0 duplicates while Go and Rust found 696, indicating a possible bug in the Python version's duplicate detection logic
- Go shows the best overall performance despite having slower file collection than Rust

## Evaluation & Recommendations

### Strengths

- **Practical value**: High — solves a real problem of finding duplicates across multiple directories and drives
- **Safety**: Good — generates a deletion script for review rather than deleting directly
- **Performance**: All three implementations use parallel processing effectively
- **Transparency**: CSV reports allow for detailed analysis before deletion

### Known Issues

1. **Python discrepancy**: The Python version found 0 duplicates while Go/Rust found 696. This needs investigation — likely related to the different file counts (149,044 vs 148,819) or a bug in the duplicate detection logic.

2. **Platform limitations**:
   - Go uses macOS-specific `syscall.Stat_t` for birth time
   - Rust uses `std::os::darwin::fs::MetadataExt` for birth time
   - Both require conditional compilation for Linux/Windows support

### Which Implementation to Use?

- **For production use on macOS**: Go — fastest overall, single binary with no dependencies
- **For cross-platform development**: Rust — easiest to adapt with `#[cfg(target_os)]` attributes
- **For quick scripting/prototyping**: Python — easiest to modify, but investigate the duplicate detection bug first

## Future Enhancements

1. **Fix Python duplicate detection** — Investigate the discrepancy between implementations
2. **Progress bar** — Add real-time progress indication during hashing phase
3. **Cross-platform birth time** — Use conditional compilation for Linux/Windows
4. **Partial hashing optimization** — Hash first/last N KB + size before full file hash
5. **Configurable output** — Allow specifying output directory and file prefixes
6. **Interactive mode** — Simple TUI for reviewing duplicates before deletion
7. **Dry-run mode** — Show what would be deleted without generating a script
8. **Move instead of delete** — Option to move duplicates to a staging directory
9. **Minimum size filter** — Skip files below a configurable threshold (e.g., <1KB)
10. **Symlink/hardlink deduplication** — Replace duplicates with hardlinks to save space without deleting

## License

This project is provided as-is for educational and practical use.

## Contributing

Contributions are welcome, especially for:
- Fixing the Python duplicate detection issue
- Adding Windows/Linux compatibility
- Implementing any of the future enhancements listed above
