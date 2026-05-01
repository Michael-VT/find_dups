#!/usr/bin/env node

const fs = require('fs').promises;
const fsSync = require('fs');
const path = require('path');
const crypto = require('crypto');
const { Worker, isMainThread, parentPort, workerData } = require('worker_threads');
const os = require('os');

const NUM_WORKERS = os.cpus().length;
const BUFFER_SIZE = 65536;

const EXTENSION_CATEGORIES = {
    '.c': 'source', '.h': 'source', '.cpp': 'source', '.hpp': 'source', '.cc': 'source',
    '.cxx': 'source', '.m': 'source', '.mm': 'source', '.s': 'source', '.S': 'source',
    '.java': 'source', '.kt': 'source', '.py': 'source', '.js': 'source', '.ts': 'source',
    '.rs': 'source', '.go': 'source', '.rb': 'source', '.swift': 'source', '.sh': 'source',
    '.hex': 'firmware', '.bin': 'firmware', '.elf': 'firmware', '.dfu': 'firmware', '.flash': 'firmware', '.map': 'firmware',
    '.uvprojx': 'ide', '.uvoptx': 'ide', '.ewp': 'ide', '.eww': 'ide', '.ewt': 'ide',
    '.cproject': 'ide', '.project': 'ide', '.mxproject': 'ide', '.ioc': 'ide',
    '.yaml': 'config', '.yml': 'config', '.cmake': 'config', '.json': 'config', '.xml': 'config',
    '.conf': 'config', '.cfg': 'config', '.ini': 'config', '.toml': 'config', '.properties': 'config',
    '.pdf': 'docs', '.md': 'docs', '.txt': 'docs', '.html': 'docs', '.htm': 'docs',
    '.rst': 'docs', '.chm': 'docs', '.doc': 'docs', '.docx': 'docs', '.rtf': 'docs',
    '.png': 'image', '.jpg': 'image', '.jpeg': 'image', '.gif': 'image', '.webp': 'image',
    '.svg': 'image', '.bmp': 'image', '.ico': 'image', '.tiff': 'image', '.tif': 'image',
    '.exe': 'binary', '.dll': 'binary', '.so': 'binary', '.dylib': 'binary', '.o': 'binary', '.a': 'binary',
    '.lib': 'binary', '.obj': 'binary', '.gch': 'binary', '.pch': 'binary',
    '.zip': 'archive', '.7z': 'archive', '.tar': 'archive', '.gz': 'archive', '.bz2': 'archive',
    '.xz': 'archive', '.rar': 'archive', '.tgz': 'archive',
    '.mp4': 'media', '.wav': 'media', '.avi': 'media', '.mp3': 'media', '.ogg': 'media',
    '.flac': 'media', '.mov': 'media', '.wmv': 'media',
    '.ttf': 'font', '.otf': 'font', '.woff': 'font', '.woff2': 'font',
    '.csv': 'data', '.dts': 'data', '.dtsi': 'data', '.overlay': 'data', '.ld': 'data', '.icf': 'data', '.srec': 'data'
};

function getCategory(ext) {
    return EXTENSION_CATEGORIES[ext] || 'other';
}

async function generateAnalytics(allFiles, bySize, elapsed) {
    const catMap = {};
    const extMap = {};
    let totalSize = 0, dupFiles = 0, dupBytes = 0;
    const sizeBins = { '0_bytes': 0, under_1kb: 0, '1kb_100kb': 0, '100kb_1mb': 0, '1mb_100mb': 0, over_100mb: 0 };

    for (const f of allFiles) {
        const ext = path.extname(f.path).toLowerCase();
        const cat = getCategory(ext);
        if (!catMap[cat]) catMap[cat] = { count: 0, total_bytes: 0, duplicate_count: 0, duplicate_bytes: 0, extensions: {} };
        catMap[cat].count++;
        catMap[cat].total_bytes += f.size;
        catMap[cat].extensions[ext] = (catMap[cat].extensions[ext] || 0) + 1;
        if (!extMap[ext]) extMap[ext] = { count: 0, total_bytes: 0, duplicate_count: 0, duplicate_bytes: 0 };
        extMap[ext].count++;
        extMap[ext].total_bytes += f.size;
        totalSize += f.size;
        if (f.size === 0) sizeBins['0_bytes']++;
        else if (f.size < 1024) sizeBins.under_1kb++;
        else if (f.size < 102400) sizeBins['1kb_100kb']++;
        else if (f.size < 1048576) sizeBins['100kb_1mb']++;
        else if (f.size < 104857600) sizeBins['1mb_100mb']++;
        else sizeBins.over_100mb++;
    }

    // Identify duplicates
    for (const [size, group] of bySize.entries()) {
        if (group.length < 2) continue;
        const hashGroups = new Map();
        for (const f of group) {
            if (f.hash) {
                if (!hashGroups.has(f.hash)) hashGroups.set(f.hash, []);
                hashGroups.get(f.hash).push(f);
            }
        }
        for (const [hash, same] of hashGroups.entries()) {
            if (same.length < 2) continue;
            for (let i = 1; i < same.length; i++) {
                const f = same[i];
                const ext = path.extname(f.path).toLowerCase();
                const cat = getCategory(ext);
                catMap[cat].duplicate_count++;
                catMap[cat].duplicate_bytes += f.size;
                extMap[ext].duplicate_count++;
                extMap[ext].duplicate_bytes += f.size;
                dupFiles++;
                dupBytes += f.size;
            }
        }
    }

    const analytics = {
        summary: {
            total_files: allFiles.length,
            total_size_bytes: totalSize,
            duplicate_files: dupFiles,
            duplicate_size_bytes: dupBytes,
            recoverable_bytes: dupBytes,
            scan_duration_seconds: parseFloat(elapsed.toFixed(3))
        },
        by_category: catMap,
        by_extension: extMap,
        size_distribution: sizeBins
    };

    await fs.writeFile('analytics_js.json', JSON.stringify(analytics, null, 2));

    console.log('\n--- File Type Analytics ---');
    for (const [cat, stats] of Object.entries(catMap)) {
        console.log(`  ${cat.padEnd(10)}: ${stats.count} files, ${stats.duplicate_count} duplicates`);
    }
    console.log('Analytics written to analytics_js.json');
}

// ---------- Utility functions ----------
function csvEscape(s) {
    const str = String(s);
    if (str.includes(',') || str.includes('"') || str.includes('\n')) {
        return '"' + str.replace(/"/g, '""') + '"';
    }
    return str;
}

function formatTime(ts) {
    const d = new Date(ts);
    return d.toISOString().replace(/\.\d+Z$/, 'Z');
}

function computeSha256(filePath) {
    return new Promise((resolve, reject) => {
        const hash = crypto.createHash('sha256');
        const stream = fsSync.createReadStream(filePath, { highWaterMark: BUFFER_SIZE });
        stream.on('data', chunk => hash.update(chunk));
        stream.on('end', () => resolve(hash.digest('hex')));
        stream.on('error', reject);
    });
}

function formatDuration(seconds) {
    const msTotal = Math.floor(seconds * 1000);
    const ms = msTotal % 1000;
    let s = Math.floor(msTotal / 1000);
    const m = Math.floor(s / 60);
    s %= 60;
    const h = Math.floor(m / 60);
    const mm = m % 60;
    return `${h.toString().padStart(2,'0')}:${mm.toString().padStart(2,'0')}:${s.toString().padStart(2,'0')}.${ms.toString().padStart(3,'0')}`;
}

// Recursive collection of regular files (no symlinks)
async function collectFiles(roots) {
    const allFiles = [];
    let nextId = 1;
    let scanned = 0;

    async function walk(dir) {
        let entries;
        try {
            entries = await fs.readdir(dir, { withFileTypes: true });
        } catch (err) {
            console.warn(`Warning: could not read ${dir}: ${err.message}`);
            return;
        }
        for (const entry of entries) {
            const fullPath = path.join(dir, entry.name);
            if (entry.isDirectory()) {
                await walk(fullPath);
            } else if (entry.isFile() && !entry.isSymbolicLink()) {
                try {
                    const stat = await fs.lstat(fullPath);
                    if (!stat.isFile()) continue;
                    const size = stat.size;
                    if (size === 0) continue; // skip zero-byte files
                    const modTime = stat.mtimeMs;
                    const birthTime = stat.birthtimeMs;
                    allFiles.push({
                        id: nextId++,
                        path: fullPath,
                        size: size,
                        modTime: modTime,
                        birthTime: birthTime,
                        hash: null
                    });
                    scanned++;
                } catch (err) {
                    // access error, skip
                }
            }
        }
    }

    for (const root of roots) {
        const absoluteRoot = path.resolve(root);
        try {
            await fs.access(absoluteRoot);
            await walk(absoluteRoot);
        } catch (err) {
            console.warn(`Warning: directory ${absoluteRoot} inaccessible: ${err.message}`);
        }
    }
    return { allFiles, scanned };
}

// Parallel hashing via worker_threads
async function parallelHash(filesToHash) {
    const total = filesToHash.length;
    if (total === 0) return;

    console.log(`Parallel hashing ${total} files (workers: ${NUM_WORKERS})...`);
    const startHash = Date.now();

    const chunkSize = Math.ceil(total / NUM_WORKERS);
    const chunks = [];
    for (let i = 0; i < total; i += chunkSize) {
        chunks.push(filesToHash.slice(i, i + chunkSize));
    }

    const results = await Promise.all(chunks.map(chunk => {
        return new Promise((resolve, reject) => {
            const worker = new Worker(__filename, {
                workerData: { chunk, type: 'hash' }
            });
            worker.on('message', resolve);
            worker.on('error', reject);
            worker.on('exit', (code) => {
                if (code !== 0) reject(new Error(`Worker stopped with exit code ${code}`));
            });
        });
    }));

    const hashMap = new Map();
    for (const resultChunk of results) {
        for (const f of resultChunk) {
            hashMap.set(f.id, f.hash);
        }
    }
    for (const f of filesToHash) {
        const h = hashMap.get(f.id);
        if (h) f.hash = h;
    }
    console.log(`Hashing completed in ${((Date.now() - startHash) / 1000).toFixed(3)} seconds`);
}

// Worker thread for hashing
if (!isMainThread) {
    const { chunk, type } = workerData;
    if (type === 'hash') {
        (async () => {
            for (const f of chunk) {
                f.hash = await computeSha256(f.path);
            }
            parentPort.postMessage(chunk);
        })().catch(err => {
            console.error(err);
            process.exit(1);
        });
    }
    return;
}

// ---------- Main function ----------
async function main() {
    const args = process.argv.slice(2);
    if (args.length === 0) {
        console.error('Usage: node find_dups.js <dir1> [<dir2> ...]');
        process.exit(1);
    }

    const startTotal = Date.now();

    // 1. Collect files
    process.stdout.write('Collecting files... ');
    const { allFiles, scanned } = await collectFiles(args);
    console.log(`found ${scanned} files`);
    if (scanned === 0) return;

    // 2. Group by size
    const bySize = new Map();
    for (const f of allFiles) {
        if (!bySize.has(f.size)) bySize.set(f.size, []);
        bySize.get(f.size).push(f);
    }

    // 3. Files to hash (size not unique)
    const filesToHash = [];
    for (const group of bySize.values()) {
        if (group.length > 1) filesToHash.push(...group);
    }

    if (filesToHash.length) {
        await parallelHash(filesToHash);
    }

    // 5. Build duplicate groups
    const csvRows = [];
    const bashLines = ['#!/bin/bash', '# Generated by find_dups_js', 'set -e', ''];
    let duplicatesCount = 0;

    for (const [size, group] of bySize.entries()) {
        if (group.length < 2) continue;
        const hashGroups = new Map();
        for (const f of group) {
            if (f.hash) {
                if (!hashGroups.has(f.hash)) hashGroups.set(f.hash, []);
                hashGroups.get(f.hash).push(f);
            }
        }
        for (const [hash, same] of hashGroups.entries()) {
            if (same.length < 2) continue;
            same.sort((a,b) => a.id - b.id);
            for (const f of same) {
                csvRows.push([
                    f.id,
                    csvEscape(f.path),
                    size,
                    hash,
                    formatTime(f.birthTime),
                    formatTime(f.modTime)
                ]);
            }
            for (let i = 1; i < same.length; i++) {
                duplicatesCount++;
                const escaped = same[i].path.replace(/'/g, "'\\''");
                bashLines.push(`rm -- '${escaped}'`);
            }
        }
    }
    bashLines.push('', 'echo "Deletion complete."');

    // 6. Write CSV and sh
    const csvHeader = ['FileID', 'Path', 'Size', 'Hash', 'CreationTime', 'ModificationTime'];
    const csvContent = [csvHeader, ...csvRows].map(row => row.join(',')).join('\n');
    await fs.writeFile('duplicates_js.csv', csvContent);
    await fs.writeFile('duprm_js.sh', bashLines.join('\n'));
    await fs.chmod('duprm_js.sh', 0o755);

    // 7. Sorted CSV of all files
    const sorted = [...allFiles].sort((a,b) => b.size - a.size);
    const sortRows = sorted.map(f => [
        f.id, csvEscape(f.path), f.size, f.hash || '',
        formatTime(f.birthTime), formatTime(f.modTime)
    ]);
    const sortContent = [csvHeader, ...sortRows].map(row => row.join(',')).join('\n');
    await fs.writeFile('sort_dup_js.csv', sortContent);

    // 8. Analytics
    const elapsed = (Date.now() - startTotal) / 1000;
    await generateAnalytics(allFiles, bySize, elapsed);

    // 9. Statistics
    console.log(`\nFiles scanned: ${scanned}`);
    console.log(`Duplicates found (files to delete): ${duplicatesCount}`);
    console.log(`Runtime:`);
    console.log(`  - ${elapsed.toFixed(3)} seconds`);
    console.log(`  - ${formatDuration(elapsed)}`);
    console.log(`Reports: duplicates_js.csv, sort_dup_js.csv`);
    console.log(`Delete script: duprm_js.sh`);
}

main().catch(err => {
    console.error(err);
    process.exit(1);
});
