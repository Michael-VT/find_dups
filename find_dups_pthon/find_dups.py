#!/usr/bin/env python3
"""find_dups.py - Duplicate file finder (regular files only, no symlinks)."""

import os
import sys
import hashlib
import csv
import json
import time
from multiprocessing import Pool, cpu_count
from collections import defaultdict

BUFFER_SIZE = 64 * 1024
NUM_WORKERS = cpu_count()

EXTENSION_CATEGORIES = {
    '.c': 'source', '.h': 'source', '.cpp': 'source', '.hpp': 'source',
    '.cc': 'source', '.cxx': 'source', '.m': 'source', '.mm': 'source',
    '.s': 'source', '.S': 'source', '.java': 'source', '.kt': 'source',
    '.py': 'source', '.js': 'source', '.ts': 'source', '.rs': 'source',
    '.go': 'source', '.rb': 'source', '.swift': 'source', '.sh': 'source',
    '.hex': 'firmware', '.bin': 'firmware', '.elf': 'firmware', '.dfu': 'firmware',
    '.flash': 'firmware', '.map': 'firmware',
    '.uvprojx': 'ide', '.uvoptx': 'ide', '.ewp': 'ide', '.eww': 'ide',
    '.ewt': 'ide', '.cproject': 'ide', '.project': 'ide', '.mxproject': 'ide',
    '.ioc': 'ide',
    '.yaml': 'config', '.yml': 'config', '.cmake': 'config', '.json': 'config',
    '.xml': 'config', '.conf': 'config', '.cfg': 'config', '.ini': 'config',
    '.toml': 'config', '.properties': 'config',
    '.pdf': 'docs', '.md': 'docs', '.txt': 'docs', '.html': 'docs',
    '.htm': 'docs', '.rst': 'docs', '.chm': 'docs', '.doc': 'docs',
    '.docx': 'docs', '.rtf': 'docs',
    '.png': 'image', '.jpg': 'image', '.jpeg': 'image', '.gif': 'image',
    '.webp': 'image', '.svg': 'image', '.bmp': 'image', '.ico': 'image',
    '.tiff': 'image', '.tif': 'image',
    '.exe': 'binary', '.dll': 'binary', '.so': 'binary', '.dylib': 'binary',
    '.o': 'binary', '.a': 'binary', '.lib': 'binary', '.obj': 'binary',
    '.gch': 'binary', '.pch': 'binary',
    '.zip': 'archive', '.7z': 'archive', '.tar': 'archive', '.gz': 'archive',
    '.bz2': 'archive', '.xz': 'archive', '.rar': 'archive', '.tgz': 'archive',
    '.mp4': 'media', '.wav': 'media', '.avi': 'media', '.mp3': 'media',
    '.ogg': 'media', '.flac': 'media', '.mov': 'media', '.wmv': 'media',
    '.ttf': 'font', '.otf': 'font', '.woff': 'font', '.woff2': 'font',
    '.csv': 'data', '.dts': 'data', '.dtsi': 'data', '.overlay': 'data',
    '.ld': 'data', '.icf': 'data', '.srec': 'data',
}

def is_regular_file(path):
    try:
        st = os.lstat(path)
        return (st.st_mode & 0o170000) == 0o100000  # S_IFREG
    except OSError:
        return False

def collect_files(roots):
    files = []
    next_id = 1
    for root in roots:
        root_path = os.path.abspath(os.path.expanduser(root))
        if not os.path.exists(root_path):
            print(f"Warning: {root_path} does not exist", file=sys.stderr)
            continue
        for dirpath, dirnames, filenames in os.walk(root_path, followlinks=False):
            for fn in filenames:
                full = os.path.join(dirpath, fn)
                if is_regular_file(full):
                    try:
                        st = os.lstat(full)
                    except OSError:
                        continue
                    if st.st_size == 0:
                        continue  # skip zero-byte files
                    files.append({
                        'id': next_id,
                        'path': full,
                        'size': st.st_size,
                        'mtime': st.st_mtime,
                        'ctime': st.st_birthtime if hasattr(st, 'st_birthtime') else st.st_ctime,
                        'hash': None
                    })
                    next_id += 1
    return files

def compute_sha256(args):
    file_path, _ = args
    try:
        sha = hashlib.sha256()
        with open(file_path, 'rb') as f:
            for chunk in iter(lambda: f.read(BUFFER_SIZE), b''):
                sha.update(chunk)
        return sha.hexdigest()
    except Exception:
        return None

def parallel_hash_chunk(chunk):
    results = []
    for f in chunk:
        f['hash'] = compute_sha256((f['path'], None))
        results.append((f['id'], f['hash']))
    return results

def format_duration(seconds):
    ms = int(seconds * 1000)
    s, ms = divmod(ms, 1000)
    m, s = divmod(s, 60)
    h, m = divmod(m, 60)
    return f"{h:02d}:{m:02d}:{s:02d}.{ms:03d}"

def write_csv(filepath, rows, header):
    try:
        with open(filepath, 'w', newline='', encoding='utf-8') as f:
            writer = csv.writer(f)
            writer.writerow(header)
            writer.writerows(rows)
        return True
    except Exception as e:
        print(f"Error writing {filepath}: {e}", file=sys.stderr)
        return False

def generate_analytics(all_files, dup_groups, elapsed):
    """Generate analytics JSON and print summary."""
    # Build set of duplicate file ids (files to delete = all but first in each group)
    dup_ids = set()
    for _size, group, _h in dup_groups:
        for f in group[1:]:
            dup_ids.add(f['id'])

    total_size = sum(f['size'] for f in all_files)
    dup_size = sum(f['size'] for f in all_files if f['id'] in dup_ids)

    # Helper to get extension and category
    def get_ext(file_path):
        basename = os.path.basename(file_path)
        dot = basename.rfind('.')
        if dot > 0:
            return basename[dot:].lower()
        elif dot == 0 and len(basename) > 1:
            return basename.lower()
        return '(none)'

    # by_extension
    by_ext = defaultdict(lambda: {'count': 0, 'total_bytes': 0, 'duplicate_count': 0, 'duplicate_bytes': 0})
    for f in all_files:
        ext = get_ext(f['path'])
        e = by_ext[ext]
        e['count'] += 1
        e['total_bytes'] += f['size']
        if f['id'] in dup_ids:
            e['duplicate_count'] += 1
            e['duplicate_bytes'] += f['size']

    # by_category
    by_cat = defaultdict(lambda: {'count': 0, 'total_bytes': 0, 'duplicate_count': 0, 'duplicate_bytes': 0, 'extensions': {}})
    for ext, edata in by_ext.items():
        cat = EXTENSION_CATEGORIES.get(ext, 'other')
        c = by_cat[cat]
        c['count'] += edata['count']
        c['total_bytes'] += edata['total_bytes']
        c['duplicate_count'] += edata['duplicate_count']
        c['duplicate_bytes'] += edata['duplicate_bytes']
        c['extensions'][ext] = edata

    # size_distribution
    size_bins = {'0_bytes': 0, 'under_1kb': 0, '1kb_100kb': 0, '100kb_1mb': 0, '1mb_100mb': 0, 'over_100mb': 0}
    for f in all_files:
        s = f['size']
        if s == 0:
            size_bins['0_bytes'] += 1
        elif s < 1024:
            size_bins['under_1kb'] += 1
        elif s < 100 * 1024:
            size_bins['1kb_100kb'] += 1
        elif s < 1024 * 1024:
            size_bins['100kb_1mb'] += 1
        elif s < 100 * 1024 * 1024:
            size_bins['1mb_100mb'] += 1
        else:
            size_bins['over_100mb'] += 1

    analytics = {
        'summary': {
            'total_files': len(all_files),
            'total_size_bytes': total_size,
            'duplicate_files': len(dup_ids),
            'duplicate_size_bytes': dup_size,
            'recoverable_bytes': dup_size,
            'scan_duration_seconds': round(elapsed, 3),
        },
        'by_category': dict(by_cat),
        'by_extension': dict(by_ext),
        'size_distribution': size_bins,
    }

    with open('analytics_py.json', 'w') as jf:
        json.dump(analytics, jf, indent=2)

    # Print human-readable summary
    print("\n--- Analytics Summary ---")
    print(f"Total files: {len(all_files)}  |  Total size: {total_size:,} bytes")
    print(f"Duplicate files (to delete): {len(dup_ids)}  |  Recoverable: {dup_size:,} bytes")
    print(f"Scan duration: {elapsed:.3f}s")
    print("\nBy category:")
    for cat in sorted(by_cat):
        c = by_cat[cat]
        print(f"  {cat:12s}  {c['count']:6d} files  {c['total_bytes']:>14,} bytes  "
              f"({c['duplicate_count']} dups, {c['duplicate_bytes']:,} bytes)")
    print("\nBy extension (top 15 by count):")
    sorted_exts = sorted(by_ext.items(), key=lambda x: x[1]['count'], reverse=True)[:15]
    for ext, edata in sorted_exts:
        print(f"  {ext:12s}  {edata['count']:6d} files  {edata['total_bytes']:>14,} bytes  "
              f"({edata['duplicate_count']} dups)")
    print(f"\nSize distribution: {dict(size_bins)}")
    print("Analytics saved: analytics_py.json")

def main():
    if len(sys.argv) < 2:
        print("Usage: python3 find_dups.py <dir1> [<dir2> ...]")
        sys.exit(1)

    start_total = time.time()
    roots = sys.argv[1:]

    print("Collecting files...", end=' ', flush=True)
    all_files = collect_files(roots)
    total_scanned = len(all_files)
    print(f"found {total_scanned} files")
    if total_scanned == 0:
        return

    # Group by size
    by_size = defaultdict(list)
    for f in all_files:
        by_size[f['size']].append(f)

    # Files to hash (size not unique)
    files_to_hash = []
    for group in by_size.values():
        if len(group) > 1:
            files_to_hash.extend(group)
    total_to_hash = len(files_to_hash)

    if total_to_hash > 0:
        print(f"Parallel hashing {total_to_hash} files (workers: {NUM_WORKERS})...")
        start_hash = time.time()

        chunk_size = max(1, total_to_hash // (NUM_WORKERS * 4))
        chunks = [files_to_hash[i:i+chunk_size] for i in range(0, total_to_hash, chunk_size)]

        with Pool(NUM_WORKERS) as pool:
            results = pool.map(parallel_hash_chunk, chunks)

        # Apply hashes from workers
        hash_map = {}
        for chunk_result in results:
            for fid, fhash in chunk_result:
                hash_map[fid] = fhash
        for f in files_to_hash:
            if f['id'] in hash_map:
                f['hash'] = hash_map[f['id']]

        print(f"Hashing completed in {format_duration(time.time() - start_hash)}")

    # Build duplicate groups
    dup_groups = []
    for size, group in by_size.items():
        if len(group) < 2:
            continue
        hash_groups = defaultdict(list)
        for f in group:
            if f['hash']:
                hash_groups[f['hash']].append(f)
        for h, g in hash_groups.items():
            if len(g) > 1:
                g.sort(key=lambda x: x['id'])
                dup_groups.append((size, g, h))

    # CSV dups
    dup_rows = []
    bash_lines = ["#!/bin/bash", "# Generated by find_dups.py", "set -e", ""]
    dups_to_delete = 0
    for size, group, h in dup_groups:
        for f in group:
            ctime = time.strftime('%Y-%m-%dT%H:%M:%S', time.localtime(f['ctime'])) + 'Z'
            mtime = time.strftime('%Y-%m-%dT%H:%M:%S', time.localtime(f['mtime'])) + 'Z'
            dup_rows.append([f['id'], f['path'], size, h, ctime, mtime])
        for f in group[1:]:
            dups_to_delete += 1
            escaped = f['path'].replace("'", "'\\''")
            bash_lines.append(f"rm -- '{escaped}'")
    bash_lines.append('')
    bash_lines.append('echo "Deletion complete."')

    write_csv('duplicates_py.csv', dup_rows,
              ['FileID', 'Path', 'Size', 'Hash', 'CreationTime', 'ModificationTime'])
    with open('duprm_py.sh', 'w') as f:
        f.write('\n'.join(bash_lines))
    os.chmod('duprm_py.sh', 0o755)

    # Sort CSV
    sorted_files = sorted(all_files, key=lambda x: x['size'], reverse=True)
    sort_rows = []
    for f in sorted_files:
        ctime = time.strftime('%Y-%m-%dT%H:%M:%S', time.localtime(f['ctime'])) + 'Z'
        mtime = time.strftime('%Y-%m-%dT%H:%M:%S', time.localtime(f['mtime'])) + 'Z'
        sort_rows.append([f['id'], f['path'], f['size'], f['hash'] or '', ctime, mtime])
    write_csv('sort_dup_py.csv', sort_rows,
              ['FileID', 'Path', 'Size', 'Hash', 'CreationTime', 'ModificationTime'])

    elapsed = time.time() - start_total
    generate_analytics(all_files, dup_groups, elapsed)
    print(f"\nFiles scanned: {total_scanned}")
    print(f"Duplicates found (files to delete): {dups_to_delete}")
    print(f"Runtime:")
    print(f"  - {elapsed:.3f} seconds")
    print(f"  - {format_duration(elapsed)}")
    print("Reports: duplicates_py.csv, sort_dup_py.csv")
    print("Delete script: duprm_py.sh")

if __name__ == '__main__':
    main()
