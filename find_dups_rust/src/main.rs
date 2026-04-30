use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use walkdir::WalkDir;
use sha2::{Sha256, Digest};
use chrono::{DateTime, Local};
use std::os::darwin::fs::MetadataExt;
use rayon::prelude::*;

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
        eprintln!("Использование: find_dups_rust <директория1> [директория2 ...]");
        std::process::exit(1);
    }
    let roots = &args[1..];

    let start_total = std::time::Instant::now();

    // ---------- 1. Сбор всех файлов ----------
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
                UNIX_EPOCH + Duration::new(birth_sec as u64, birth_nsec as u32)
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

    // Группировка по размеру (используем ссылку, чтобы не перемещать all_files)
    let mut by_size: HashMap<u64, Vec<FileInfo>> = HashMap::new();
    for f in &all_files {
        by_size.entry(f.size).or_default().push(f.clone());
    }

    // Собираем файлы для хеширования (те, размер которых встречается более одного раза)
    let mut files_to_hash = Vec::new();
    for files in by_size.values() {
        if files.len() > 1 {
            files_to_hash.extend(files.iter().cloned());
        }
    }
    let total_to_hash = files_to_hash.len();

    // ---------- 2. Параллельное вычисление хешей (rayon) ----------
    if total_to_hash > 0 {
        println!("Параллельное хеширование {} файлов (потоков: {})...", total_to_hash, rayon::current_num_threads());
        let start_hash = std::time::Instant::now();

        files_to_hash.par_iter_mut().for_each(|f| {
            if let Ok(hash) = compute_sha256(&f.path) {
                f.hash = hash;
            } else {
                eprintln!("Ошибка при чтении {:?}", f.path);
            }
        });

        println!("Хеширование завершено за {:?}", start_hash.elapsed());
    }

    // Обновляем хеши в by_size (используем map для быстрого доступа)
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

    // ---------- 3. CSV для дубликатов (duplicates_rs.csv) ----------
    let mut csv_writer = csv::Writer::from_path("duplicates_rs.csv")?;
    csv_writer.write_record(&["FileID", "Path", "Size", "Hash", "CreationTime", "ModificationTime"])?;

    let mut sh_file = File::create("duprm_rs.sh")?;
    writeln!(sh_file, "#!/bin/bash\n# Сгенерировано find_dups_rs\nset -e\n")?;

    let mut duplicates_count = 0;

    for (size, files) in by_size {
        if files.len() < 2 {
            continue;
        }
        let mut hash_groups: HashMap<String, Vec<&FileInfo>> = HashMap::new();
        for f in files.iter() {
            hash_groups.entry(f.hash.clone()).or_default().push(f);
        }
        for (hash, mut group) in hash_groups {
            if group.len() < 2 {
                continue;
            }
            group.sort_by_key(|f| f.id);
            for dup in &group {
                let birth_str = DateTime::<Local>::from(dup.created).to_rfc3339();
                let mod_str = DateTime::<Local>::from(dup.modified).to_rfc3339();
                csv_writer.write_record(&[
                    dup.id.to_string(),
                    dup.path.to_string_lossy().to_string(),
                    size.to_string(),
                    hash.clone(),
                    birth_str,
                    mod_str,
                ])?;
            }
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
        fs::set_permissions("duprm_rs.sh", perms)?;
    }

    // ---------- 4. Сортированный CSV всех файлов (sort_dup_rs.csv) ----------
    let mut sorted_files = all_files.clone();
    sorted_files.sort_by(|a, b| b.size.cmp(&a.size)); // по убыванию размера

    let mut sort_writer = csv::Writer::from_path("sort_dup_rs.csv")?;
    sort_writer.write_record(&["FileID", "Path", "Size", "Hash", "CreationTime", "ModificationTime"])?;
    for f in sorted_files {
        let birth_str = DateTime::<Local>::from(f.created).to_rfc3339();
        let mod_str = DateTime::<Local>::from(f.modified).to_rfc3339();
        sort_writer.write_record(&[
            f.id.to_string(),
            f.path.to_string_lossy().to_string(),
            f.size.to_string(),
            f.hash,
            birth_str,
            mod_str,
        ])?;
    }
    sort_writer.flush()?;

    // ---------- 5. Вывод статистики ----------
    let elapsed = start_total.elapsed();
    println!("\nПросмотрено файлов: {}", scanned);
    println!("Найдено дубликатов (файлов для удаления): {}", duplicates_count);
    println!("Время работы:");
    println!("  - {:.3} секунд", elapsed.as_secs_f64());
    println!("  - {}", format_duration(elapsed));
    println!("Отчёты: duplicates_rs.csv, sort_dup_rs.csv");
    println!("Скрипт удаления: duprm_rs.sh");

    Ok(())
}

fn compute_sha256(path: &std::path::Path) -> Result<String, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0; 16384];
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
