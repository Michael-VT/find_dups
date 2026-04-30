#!/usr/bin/env python3
"""
find_dups.py - поиск дубликатов файлов в нескольких директориях.
Создаёт:
- duplicates.csv  : все файлы-дубликаты (группы с одинаковым содержимым)
- sort_dup.csv    : все найденные файлы, отсортированные по размеру (убывание)
- duprm.sh        : bash-скрипт для удаления дубликатов (сохраняется первый файл в группе по ID)
"""

import os
import sys
import hashlib
import csv
import time
from pathlib import Path
from multiprocessing import Pool, cpu_count
from functools import partial
from collections import defaultdict

# ------------------------------------------------------------
# Конфигурация
BUFFER_SIZE = 64 * 1024       # 64 KB – для быстрого чтения
NUM_WORKERS = cpu_count()     # количество процессов для хеширования

# ------------------------------------------------------------
def collect_files(roots):
    """Рекурсивный обход директорий, возвращает список словарей с информацией о файлах."""
    all_files = []
    next_id = 1
    for root in roots:
        root_path = Path(root).expanduser().resolve()
        if not root_path.exists():
            print(f"Предупреждение: директория {root_path} не существует, пропускаем", file=sys.stderr)
            continue
        for entry in root_path.rglob('*'):
            if entry.is_file():
                try:
                    stat = entry.stat()
                except OSError:
                    continue      # игнорируем ошибки доступа
                all_files.append({
                    'id': next_id,
                    'path': str(entry),
                    'size': stat.st_size,
                    'mtime': stat.st_mtime,
                    'ctime': stat.st_birthtime if hasattr(stat, 'st_birthtime') else stat.st_ctime,
                    'hash': None
                })
                next_id += 1
    return all_files

def compute_sha256(file_path):
    """Вычисляет SHA256 хеш файла, возвращает hex-строку."""
    sha = hashlib.sha256()
    try:
        with open(file_path, 'rb') as f:
            for chunk in iter(lambda: f.read(BUFFER_SIZE), b''):
                sha.update(chunk)
        return sha.hexdigest()
    except (OSError, IOError):
        return None

def parallel_hash(files_chunk):
    """Функция для пула: вычисляет хеши для списка файлов (каждый элемент – словарь)."""
    for f in files_chunk:
        f['hash'] = compute_sha256(f['path'])
    return files_chunk

def format_duration(seconds):
    """Форматирует время в чч:мм:сс.мс.
       seconds – float или int (секунды с дробной частью)."""
    ms_total = int(seconds * 1000)
    ms = ms_total % 1000
    s = ms_total // 1000
    m = s // 60
    s %= 60
    h = m // 60
    m %= 60
    return f"{h:02d}:{m:02d}:{s:02d}.{ms:03d}"

def main():
    if len(sys.argv) < 2:
        print("Использование: python find_dups_python.py <директория1> [директория2 ...]")
        sys.exit(1)

    roots = sys.argv[1:]
    start_total = time.time()

    # ---------- 1. Сбор файлов ----------
    print("Сбор файлов...", end=' ', flush=True)
    all_files = collect_files(roots)
    total_scanned = len(all_files)
    print(f"найдено {total_scanned} файлов")

    if total_scanned == 0:
        print("Нет файлов для анализа.")
        return

    # ---------- 2. Группировка по размеру ----------
    by_size = defaultdict(list)
    for f in all_files:
        by_size[f['size']].append(f)

    # Выявляем файлы, для которых нужно вычислить хеш (размер не уникален)
    files_to_hash = []
    for size_group in by_size.values():
        if len(size_group) > 1:
            files_to_hash.extend(size_group)

    # ---------- 3. Параллельное хеширование ----------
    if files_to_hash:
        print(f"Параллельное хеширование {len(files_to_hash)} файлов (воркеров: {NUM_WORKERS})...", flush=True)
        start_hash = time.time()

        # Разбиваем список на чанки для пула
        chunk_size = max(1, len(files_to_hash) // (NUM_WORKERS * 4))
        chunks = [files_to_hash[i:i+chunk_size] for i in range(0, len(files_to_hash), chunk_size)]

        with Pool(NUM_WORKERS) as pool:
            results = pool.map(parallel_hash, chunks)
        # Обновляем хеши в исходных файлах (результаты уже изменены, но нужно объединить)
        # Однако parallel_hash изменяет объекты напрямую, поэтому всё уже на месте.
        print(f"Хеширование завершено за {format_duration(time.time() - start_hash)}")

    # Проверка: для всех файлов с неуникальным размером должен быть заполнен hash
    # (если какой-то не удалось прочитать – hash останется None, пропустим его)

    # ---------- 4. Подготовка к формированию CSV и bash-скрипта ----------
    # Группировка по хешу внутри каждой размерной группы
    # Сначала для каждого размера создаём группы по хешу
    duplicate_groups = []  # список кортежей (size, list_of_FileInfo)
    for size, group in by_size.items():
        if len(group) < 2:
            continue
        # группируем по хешу, отбрасывая файлы с None-хешем
        hash_groups = defaultdict(list)
        for f in group:
            if f['hash'] is not None:
                hash_groups[f['hash']].append(f)
        for h, hgroup in hash_groups.items():
            if len(hgroup) > 1:
                # сортируем по ID
                hgroup.sort(key=lambda x: x['id'])
                duplicate_groups.append((size, hgroup, h))

    # ---------- 5. Запись duplicates.csv и duprm.sh ----------
    with open('duplicates_py.csv', 'w', newline='', encoding='utf-8') as csvf, \
         open('duprm_py.sh', 'w', encoding='utf-8') as shf:
        csv_writer = csv.writer(csvf)
        csv_writer.writerow(['FileID', 'Path', 'Size', 'Hash', 'CreationTime', 'ModificationTime'])

        shf.write('#!/bin/bash\n# Сгенерировано find_dups.py\nset -e\n\n')
        duplicates_count = 0

        for size, group, hash_val in duplicate_groups:
            # Записываем все файлы группы в CSV
            for f in group:
                birth_time = time.strftime('%Y-%m-%dT%H:%M:%S', time.localtime(f['ctime'])) + 'Z'
                mod_time = time.strftime('%Y-%m-%dT%H:%M:%S', time.localtime(f['mtime'])) + 'Z'
                csv_writer.writerow([
                    f['id'],
                    f['path'],
                    size,
                    hash_val,
                    birth_time,
                    mod_time
                ])
            # В bash-скрипт добавляем удаление всех, кроме первого (по ID)
            for f in group[1:]:
                duplicates_count += 1
                # экранируем кавычки и спецсимволы
                escaped_path = f['path'].replace("'", "'\\''")
                shf.write(f"rm -- '{escaped_path}'\n")
        shf.write('\necho "Удаление дубликатов завершено."\n')

    # Делаем bash-скрипт исполняемым (только на Unix)
    os.chmod('duprm.sh', 0o755)

    # ---------- 6. Сортированный файл sort_dup.csv ----------
    # Сортируем все файлы по размеру (убывание)
    sorted_files = sorted(all_files, key=lambda x: x['size'], reverse=True)
    with open('sort_dup_py.csv', 'w', newline='', encoding='utf-8') as csvf:
        csv_writer = csv.writer(csvf)
        csv_writer.writerow(['FileID', 'Path', 'Size', 'Hash', 'CreationTime', 'ModificationTime'])
        for f in sorted_files:
            birth_time = time.strftime('%Y-%m-%dT%H:%M:%S', time.localtime(f['ctime'])) + 'Z'
            mod_time = time.strftime('%Y-%m-%dT%H:%M:%S', time.localtime(f['mtime'])) + 'Z'
            csv_writer.writerow([
                f['id'],
                f['path'],
                f['size'],
                f['hash'] if f['hash'] else '',
                birth_time,
                mod_time
            ])

    # ---------- 7. Вывод статистики ----------
    elapsed = time.time() - start_total
    print(f"\nПросмотрено файлов: {total_scanned}")
    print(f"Найдено дубликатов (файлов для удаления): {duplicates_count}")
    print(f"Время работы:")
    print(f"  - {elapsed:.3f} секунд")
    print(f"  - {format_duration(elapsed)}")
    print("Отчёты: duplicates_py.csv, sort_dup_py.csv")
    print("Скрипт удаления: duprm_py.sh")

if __name__ == '__main__':
    main()

