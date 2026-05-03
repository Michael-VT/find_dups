use std::collections::HashMap;
use std::fs::{self, File, Metadata};
use std::io::{BufReader, Read, Write};
use std::os::darwin::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;
use sha2::{Sha256, Digest};
use chrono::{DateTime, Local};
use rayon::prelude::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, AtomicBool, Ordering};
use std::thread;


// ── Helper: format bytes ────────────────────────────────────────────────
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;
    
    if bytes >= TB {
        format!("{:.1} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

const BUFFER_SIZE: usize = 65536;

// ── Progress tracker ──────────────────────────────────────────────────
struct ProgressTracker {
    total: usize,
    processed: Arc<AtomicUsize>,
}

impl ProgressTracker {
    fn new(total: usize) -> Self {
        Self {
            total,
            processed: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn start(&self) {
        let processed = Arc::clone(&self.processed);
        let total = self.total;

        thread::spawn(move || {
            let spinners = ['|', '/', '-', '\\'];
            let mut spinner_idx = 0;
            
            loop {
                let current = processed.load(Ordering::Relaxed);
                if current >= total {
                    // Print final status before exiting
                    print!("\rHashing: {}/{} files {}\n", current, total, spinners[spinner_idx]);
                    use std::io::Write;
                    std::io::stdout().flush().ok();
                    break;
                }

                print!("\n\rHashing: {}/{} files {}", current, total, spinners[spinner_idx]);
                use std::io::Write;
                std::io::stdout().flush().ok();

                spinner_idx = (spinner_idx + 1) % spinners.len();
                thread::sleep(Duration::from_secs(1));
            }
        });
    }

    fn increment(&self) {
        self.processed.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Debug)]
struct FileInfo {
    id: usize,
    path: PathBuf,
    size: u64,
    modified: SystemTime,
    created: SystemTime,
    hash: String,
}

// ── Analytics types ────────────────────────────────────────────────

#[derive(serde::Serialize)]
struct Analytics {
    summary: Summary,
    by_category: HashMap<String, CategoryStats>,
    by_extension: HashMap<String, ExtensionStats>,
    size_distribution: HashMap<String, usize>,
}

#[derive(serde::Serialize)]
struct Summary {
    total_files: usize,
    total_size_bytes: u64,
    duplicate_files: usize,
    duplicate_size_bytes: u64,
    recoverable_bytes: u64,
    scan_duration_seconds: f64,
}

#[derive(serde::Serialize)]
struct CategoryStats {
    count: usize,
    total_bytes: u64,
    duplicate_count: usize,
    duplicate_bytes: u64,
    extensions: HashMap<String, usize>,
}

#[derive(serde::Serialize)]
struct ExtensionStats {
    count: usize,
    total_bytes: u64,
    duplicate_count: usize,
    duplicate_bytes: u64,
}

// ── Category mapping ──────────────────────────────────────────────

fn get_category(ext: &str) -> &'static str {
    match ext {
        // source
        "c" | "h" | "cpp" | "hpp" | "cc" | "cxx" | "m" | "mm" | "s" | "S"
        | "java" | "kt" | "py" | "js" | "ts" | "rs" | "go" | "rb" | "swift" | "sh" => "source",
        // firmware
        "hex" | "bin" | "elf" | "dfu" | "flash" | "map" => "firmware",
        // ide
        "uvprojx" | "uvoptx" | "ewp" | "eww" | "ewt"
        | "cproject" | "project" | "mxproject" | "ioc" => "ide",
        // config
        "yaml" | "yml" | "cmake" | "json" | "xml"
        | "conf" | "cfg" | "ini" | "toml" | "properties" => "config",
        // docs
        "pdf" | "md" | "txt" | "html" | "htm" | "rst"
        | "chm" | "doc" | "docx" | "rtf" => "docs",
        // image
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg"
        | "bmp" | "ico" | "tiff" | "tif" => "image",
        // binary
        "exe" | "dll" | "so" | "dylib" | "o" | "a" | "lib" | "obj" | "gch" | "pch" => "binary",
        // archive
        "zip" | "7z" | "tar" | "gz" | "bz2" | "xz" | "rar" | "tgz" => "archive",
        // media
        "mp4" | "wav" | "avi" | "mp3" | "ogg" | "flac" | "mov" | "wmv" => "media",
        // font
        "ttf" | "otf" | "woff" | "woff2" => "font",
        // data
        "csv" | "dts" | "dtsi" | "overlay" | "ld" | "icf" | "srec" => "data",
        _ => "other",
    }
}

// ── Analytics generation ──────────────────────────────────────────

fn generate_analytics(
    all_files: &[FileInfo],
    by_size: &HashMap<u64, Vec<usize>>,
    elapsed: Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    // Identify duplicate file indices (second file onward in each hash group)
    let mut dup_set: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for indices in by_size.values() {
        if indices.len() < 2 { continue; }
        let mut hash_groups: HashMap<&str, Vec<usize>> = HashMap::new();
        for &idx in indices {
            let h = &all_files[idx].hash;
            if !h.is_empty() {
                hash_groups.entry(h.as_str()).or_default().push(idx);
            }
        }
        for same in hash_groups.values() {
            if same.len() >= 2 {
                let mut sorted = same.clone();
                sorted.sort();
                for &idx in sorted.iter().skip(1) {
                    dup_set.insert(idx);
                }
            }
        }
    }

    let duplicate_files = dup_set.len();
    let duplicate_size: u64 = dup_set.iter().map(|&i| all_files[i].size).sum();

    // Collect by category and extension
    let mut cat_stats: HashMap<String, CategoryStats> = HashMap::new();
    let mut ext_stats: HashMap<String, ExtensionStats> = HashMap::new();

    for (idx, f) in all_files.iter().enumerate() {
        let ext_raw = f.path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let ext = if ext_raw.is_empty() { String::new() } else { format!(".{}", ext_raw.to_lowercase()) };
        let category = get_category(&ext_raw.to_lowercase()).to_string();
        let is_dup = dup_set.contains(&idx);

        cat_stats.entry(category.clone())
            .or_insert_with(|| CategoryStats { count: 0, total_bytes: 0, duplicate_count: 0, duplicate_bytes: 0, extensions: HashMap::new() })
            .count += 1;
        let cs = cat_stats.get_mut(&category).unwrap();
        cs.total_bytes += f.size;
        if is_dup { cs.duplicate_count += 1; cs.duplicate_bytes += f.size; }
        if !ext.is_empty() {
            *cs.extensions.entry(ext.clone()).or_insert(0) += 1;
        }

        ext_stats.entry(ext.clone())
            .or_insert_with(|| ExtensionStats { count: 0, total_bytes: 0, duplicate_count: 0, duplicate_bytes: 0 })
            .count += 1;
        let es = ext_stats.get_mut(&ext).unwrap();
        es.total_bytes += f.size;
        if is_dup { es.duplicate_count += 1; es.duplicate_bytes += f.size; }
    }

    // Size distribution
    let mut size_dist: HashMap<String, usize> = HashMap::new();
    for key in &["0_bytes", "under_1kb", "1kb_100kb", "100kb_1mb", "1mb_100mb", "over_100mb"] {
        size_dist.insert(key.to_string(), 0);
    }
    for f in all_files {
        let bin = if f.size == 0 { "0_bytes" }
                  else if f.size < 1024 { "under_1kb" }
                  else if f.size < 100 * 1024 { "1kb_100kb" }
                  else if f.size < 1024 * 1024 { "100kb_1mb" }
                  else if f.size < 100 * 1024 * 1024 { "1mb_100mb" }
                  else { "over_100mb" };
        *size_dist.get_mut(bin).unwrap() += 1;
    }

    let total_size: u64 = all_files.iter().map(|f| f.size).sum();
    let analytics = Analytics {
        summary: Summary {
            total_files: all_files.len(),
            total_size_bytes: total_size,
            duplicate_files,
            duplicate_size_bytes: duplicate_size,
            recoverable_bytes: duplicate_size,
            scan_duration_seconds: elapsed.as_secs_f64(),
        },
        by_category: cat_stats,
        by_extension: ext_stats,
        size_distribution: size_dist,
    };

    // Write JSON
    let json = serde_json::to_string_pretty(&analytics)?;
    let mut f = File::create("analytics_rs.json")?;
    f.write_all(json.as_bytes())?;

    // Human-readable summary
    println!("\n--- File Type Analytics ---");
    let mut cats: Vec<_> = analytics.by_category.iter().collect();
    cats.sort_by(|a, b| b.1.count.cmp(&a.1.count));
    for (name, stats) in &cats {
        println!("  {:12} {:6} files, {} duplicates", name, stats.count, stats.duplicate_count);
    }
    println!("Analytics written to analytics_rs.json");

    Ok(())
}

// ── Core helpers ──────────────────────────────────────────────────

fn is_regular_file(metadata: &Metadata) -> bool {
    metadata.is_file() && !metadata.is_symlink()
}

fn compute_sha256(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; BUFFER_SIZE];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 { break; }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn format_duration(d: Duration) -> String {
    let ms = d.as_millis();
    let s = ms / 1000;
    let ms_part = ms % 1000;
    let m = s / 60;
    let s_part = s % 60;
    let h = m / 60;
    let m_part = m % 60;
    format!("{:02}:{:02}:{:02}.{:03}", h, m_part, s_part, ms_part)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: find_dups <dir1> [<dir2> ...]");
        std::process::exit(1);
    }
    let roots = &args[1..];
    let start_total = std::time::Instant::now();
    // ----- 1. Collect regular files -----
    let mut all_files = Vec::new();
    let mut next_id = 1;
    let mut scanned = 0;
    
    // Start file collection progress indicator
    let collection_bytes = Arc::new(AtomicUsize::new(0));
    let collection_counter = Arc::new(AtomicUsize::new(0));
    let bytes_clone = Arc::clone(&collection_bytes);
    let counter_clone = Arc::clone(&collection_counter);
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_clone = Arc::clone(&stop_flag);
    
    thread::spawn(move || {
        while !stop_flag_clone.load(Ordering::Relaxed) {
            let count = counter_clone.load(Ordering::Relaxed);
            let bytes = bytes_clone.load(Ordering::Relaxed) as u64;
            if count > 0 {
                print!("\n\rCollecting files... {} files, {} scanned so far...", count, format_bytes(bytes));
                use std::io::Write;
                std::io::stdout().flush().ok();
            }
            thread::sleep(Duration::from_secs(5));
        }
    });

    for root in roots {
        for entry in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            if !is_regular_file(&metadata) { continue; }
            let size = metadata.len();
            if size == 0 { continue; } // skip zero-byte files
            scanned += 1;
            collection_bytes.fetch_add(size as usize, Ordering::Relaxed);
            collection_counter.store(scanned, Ordering::Relaxed);
            let path = entry.into_path();
            let modified = metadata.modified()?;
            let birth_sec = metadata.st_birthtime();
            let birth_nsec = metadata.st_birthtime_nsec();
            let birth = if birth_sec >= 0 && birth_nsec >= 0 {
                UNIX_EPOCH + Duration::new(birth_sec as u64, birth_nsec as u32)
            } else {
                modified
            };
            all_files.push(FileInfo {
                id: next_id,
                path,
                size,
                modified,
                created: birth,
                hash: String::new(),
            });
            next_id += 1;
        }
    }
    
    stop_flag.store(true, Ordering::Relaxed);
    let total_bytes = collection_bytes.load(Ordering::Relaxed) as u64;
    println!("\rCollecting files... found {} files, {}", scanned, format_bytes(total_bytes));
    if scanned == 0 { return Ok(()); }

    // ----- 2. Group by size (indices) -----
    let mut by_size: HashMap<u64, Vec<usize>> = HashMap::new();
    for (idx, f) in all_files.iter().enumerate() {
        by_size.entry(f.size).or_default().push(idx);
    }

    // ----- 3. Parallel hashing -----
    // Collect indices of files that need hashing (size groups with >1 file)
    let mut files_to_hash: Vec<usize> = Vec::new();
    for indices in by_size.values() {
        if indices.len() > 1 {
            files_to_hash.extend(indices.iter().copied());
        }
    }

    let total_to_hash = files_to_hash.len();
    if total_to_hash > 0 {
        println!(
            "Parallel hashing {} files (workers: {})...",
            total_to_hash,
            rayon::current_num_threads()
        );
        let start_hash = std::time::Instant::now();

        // Start progress tracker
        let progress = ProgressTracker::new(total_to_hash);
        progress.start();

        let hashes: Vec<(usize, String)> = files_to_hash
            .par_iter()
            .filter_map(|&idx| {
                let path = &all_files[idx].path;
                let result = compute_sha256(path).ok();
                if result.is_some() {
                    progress.increment();
                }
                result.map(|h| (idx, h))
            })
            .collect();


        for (idx, hash) in hashes {
            all_files[idx].hash = hash;
        }
        println!("\nHashing completed in {:?}", start_hash.elapsed());
    }

    // ----- 4. Build duplicate groups -----
    let mut csv_dups = Vec::new();
    let mut bash_lines = vec![
        "#!/bin/bash".to_string(),
        "# Generated by find_dups".to_string(),
        "set -e".to_string(),
        "".to_string(),
    ];
    let mut duplicates_count = 0;

    for (size, indices) in &by_size {
        if indices.len() < 2 { continue; }
        let mut hash_groups: HashMap<&str, Vec<usize>> = HashMap::new();
        for &idx in indices {
            let h = &all_files[idx].hash;
            if !h.is_empty() {
                hash_groups.entry(h.as_str()).or_default().push(idx);
            }
        }
        for (hash, mut same_indices) in hash_groups {
            if same_indices.len() < 2 { continue; }
            same_indices.sort();
            for &idx in &same_indices {
                let f = &all_files[idx];
                let birth = DateTime::<Local>::from(f.created).to_rfc3339();
                let modified = DateTime::<Local>::from(f.modified).to_rfc3339();
                csv_dups.push(vec![
                    f.id.to_string(),
                    f.path.to_string_lossy().to_string(),
                    size.to_string(),
                    hash.to_string(),
                    birth,
                    modified,
                ]);
            }
            for &idx in same_indices.iter().skip(1) {
                duplicates_count += 1;
                let escaped = all_files[idx].path.to_string_lossy().replace('\'', "'\\''");
                bash_lines.push(format!("rm -- '{}'", escaped));
            }
        }
    }
    bash_lines.push("".to_string());
    bash_lines.push("echo \"Deletion complete.\"".to_string());

    // Write CSV and sh
    let mut csv_writer = csv::Writer::from_path("duplicates_rs.csv")?;
    csv_writer.write_record(&["FileID", "Path", "Size", "Hash", "CreationTime", "ModificationTime"])?;
    for row in csv_dups { csv_writer.write_record(&row)?; }
    csv_writer.flush()?;

    let mut sh_file = File::create("duprm_rs.sh")?;
    for line in bash_lines { writeln!(sh_file, "{}", line)?; }
    #[cfg(unix)]
    { let _ = fs::set_permissions("duprm_rs.sh", fs::Permissions::from_mode(0o755)); }
    drop(sh_file);

    // ----- 5. Sorted CSV -----
    let mut sort_indices: Vec<usize> = (0..all_files.len()).collect();
    sort_indices.sort_by(|&a, &b| all_files[b].size.cmp(&all_files[a].size));
    let mut sort_writer = csv::Writer::from_path("sort_dup_rs.csv")?;
    sort_writer.write_record(&["FileID", "Path", "Size", "Hash", "CreationTime", "ModificationTime"])?;
    for &idx in &sort_indices {
        let f = &all_files[idx];
        let birth = DateTime::<Local>::from(f.created).to_rfc3339();
        let modified = DateTime::<Local>::from(f.modified).to_rfc3339();
        sort_writer.write_record(&[
            f.id.to_string(),
            f.path.to_string_lossy().to_string(),
            f.size.to_string(),
            f.hash.clone(),
            birth,
            modified,
        ])?;
    }
    sort_writer.flush()?;

    // ----- 6. Analytics -----
    let elapsed = start_total.elapsed();
    generate_analytics(&all_files, &by_size, elapsed)?;

    // ----- 7. Statistics -----
    println!("\nFiles scanned: {}", scanned);
    println!("Duplicates found (files to delete): {}", duplicates_count);
    println!("Runtime:");
    println!("  - {:.3} seconds", elapsed.as_secs_f64());
    println!("  - {}", format_duration(elapsed));
    println!("Reports: duplicates_rs.csv, sort_dup_rs.csv, analytics_rs.json");
    println!("Delete script: duprm_rs.sh");

    Ok(())
}
