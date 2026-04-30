// src/main.rs
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, Read, Write, stdout};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;
use sha2::{Sha256, Digest};
use chrono::{DateTime, Local};
use std::os::darwin::fs::MetadataExt;

#[derive(Debug, Clone)]
struct FileInfo {
    id: usize,
    path: PathBuf,
    size: u64,
    modified: SystemTime,
    created: SystemTime,
    hash: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Использование: find_dups <директория1> [директория2 ...]");
        std::process::exit(1);
    }
    let roots = &args[1..];

    let start = std::time::Instant::now();

    let mut all_files = Vec::new();
    let mut scanned = 0;
    let mut next_id = 1;

    for root in roots {
        for entry in WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            if !metadata.is_file() {
                continue;
            }
            scanned += 1;
            let path = entry.into_path();
            let size = metadata.len();
            let modified = metadata.modified()?;
            let birth_sec = metadata.st_birthtime();
            let birth_nsec = metadata.st_birthtime_nsec();
            let created = if birth_sec >= 0 && birth_nsec >= 0 {
                UNIX_EPOCH + std::time::Duration::new(birth_sec as u64, birth_nsec as u32)
            } else {
                modified
            };
            all_files.push(FileInfo {
                id: next_id,
                path,
                size,
                modified,
                created,
                hash: String::new(),
            });
            next_id += 1;
        }
    }

    // Группировка по размеру
    let mut by_size: HashMap<u64, Vec<FileInfo>> = HashMap::new();
    for f in all_files {
        by_size.entry(f.size).or_default().push(f);
    }

    // Собираем файлы, для которых будем вычислять хеш
    let mut files_to_hash = Vec::new();
    for files in by_size.values() {
        if files.len() > 1 {
            files_to_hash.extend(files.iter().cloned());
        }
    }
    let total_to_hash = files_to_hash.len();
    let mut hashed_count = 0;

    // Вычисляем хеши с прогресс-баром
    for f in &mut files_to_hash {
        let hash = compute_sha256(&f.path)?;
        f.hash = hash;
        hashed_count += 1;
        let percent = (hashed_count as f64 / total_to_hash as f64) * 100.0;
        print!("\rВычисление хешей: {}/{} ({:.1}%)", hashed_count, total_to_hash, percent);
        stdout().flush()?;
    }
    println!();

    // Обновляем хеши в by_size
    let mut hash_map = HashMap::new();
    for f in files_to_hash {
        hash_map.insert(f.path.clone(), f.hash.clone());
    }
    for (_, files) in by_size.iter_mut() {
        for f in files.iter_mut() {
            if let Some(hash) = hash_map.get(&f.path) {
                f.hash = hash.clone();
            }
        }
    }

    // CSV и bash-скрипт
    let mut csv_writer = csv::Writer::from_path("duplicates.csv")?;
    csv_writer.write_record(&["FileID", "Path", "Size", "Hash", "CreationTime", "ModificationTime"])?;

    let mut sh_file = File::create("duprm.sh")?;
    writeln!(sh_file, "#!/bin/bash\n# Сгенерировано find_dups\nset -e\n")?;

    let mut duplicates_count = 0;

    for (size, files) in by_size {
        if files.len() < 2 {
            continue;
        }
        let mut hash_groups: HashMap<String, Vec<&FileInfo>> = HashMap::new();
        for f in files.iter() {
            hash_groups.entry(f.hash.clone()).or_default().push(f);
        }
        for (hash, group) in hash_groups {
            if group.len() < 2 {
                continue;
            }
            // Записываем все файлы группы в CSV
            for dup in &group {
                let birth_str = DateTime::<Local>::from(dup.created).to_rfc3339();
                let mod_str = DateTime::<Local>::from(dup.modified).to_rfc3339();
                let record = vec![
                    dup.id.to_string(),
                    dup.path.to_string_lossy().to_string(),
                    size.to_string(),
                    hash.clone(),
                    birth_str,
                    mod_str,
                ];
                csv_writer.write_record(&record)?;
            }
            // В bash-скрипт добавляем удаление всех, кроме первого (по порядку id)
            for (idx, dup) in group.iter().enumerate() {
                if idx == 0 {
                    continue;
                }
                duplicates_count += 1;
                writeln!(sh_file, "rm -- {:?}", dup.path)?;
            }
        }
    }

    csv_writer.flush()?;
    writeln!(sh_file, "\necho \"Удаление дубликатов завершено.\"")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = sh_file.metadata()?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions("duprm.sh", perms)?;
    }

    let elapsed = start.elapsed();
    println!("Время работы: {:?}", elapsed);
    println!("Просмотрено файлов: {}", scanned);
    println!("Найдено дубликатов (файлов для удаления): {}", duplicates_count);
    println!("Отчёт сохранён в duplicates.csv, скрипт удаления – duprm.sh");

    Ok(())
}

fn compute_sha256(path: &std::path::Path) -> Result<String, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0; 8192];
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    let hash = hasher.finalize();
    Ok(format!("{:x}", hash))
}

