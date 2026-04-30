#!/usr/bin/env node

const fs = require('fs').promises;
const fsSync = require('fs');
const path = require('path');
const crypto = require('crypto');
const { Worker, isMainThread, parentPort, workerData } = require('worker_threads');
const os = require('os');

const NUM_WORKERS = os.cpus().length;
const BUFFER_SIZE = 65536;

// ---------- Вспомогательные функции ----------
function formatTime(ts) {
    const d = new Date(ts);
    return d.toISOString().replace(/\.\d+Z$/, 'Z');
}

async function computeSha256(filePath) {
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

// Рекурсивный сбор обычных файлов (без симлинков)
async function collectFiles(roots) {
    const allFiles = [];
    let nextId = 1;
    let scanned = 0;

    async function walk(dir) {
        let entries;
        try {
            entries = await fs.readdir(dir, { withFileTypes: true });
        } catch (err) {
            console.warn(`Предупреждение: не удалось прочитать ${dir}: ${err.message}`);
            return;
        }
        for (const entry of entries) {
            const fullPath = path.join(dir, entry.name);
            if (entry.isDirectory()) {
                await walk(fullPath);
            } else if (entry.isFile() && !entry.isSymbolicLink()) {
                try {
                    const stat = await fs.lstat(fullPath);
                    if (!stat.isFile()) continue; // ещё раз проверим
                    const size = stat.size;
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
                    // ошибка доступа, пропускаем
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
            console.warn(`Предупреждение: директория ${absoluteRoot} недоступна: ${err.message}`);
        }
    }
    return { allFiles, scanned };
}

// Параллельное хеширование через worker_threads
async function parallelHash(filesToHash) {
    const total = filesToHash.length;
    if (total === 0) return;

    console.log(`Параллельное хеширование ${total} файлов (воркеров: ${NUM_WORKERS})...`);
    const startHash = Date.now();

    // Разбиваем на чанки
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

    const endHash = Date.now();
    console.log(`Хеширование завершено за ${((endHash - startHash) / 1000).toFixed(3)} секунд`);

    // Обновляем хеши в исходных объектах (по id)
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
}

// Worker поток для хеширования
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

// ---------- Основная функция ----------
async function main() {
    const args = process.argv.slice(2);
    if (args.length === 0) {
        console.error('Использование: node find_dups.js <директория1> [<директория2> ...]');
        process.exit(1);
    }

    const startTotal = Date.now();

    // 1. Сбор файлов
    process.stdout.write('Сбор файлов... ');
    const { allFiles, scanned } = await collectFiles(args);
    console.log(`найдено ${scanned} файлов`);
    if (scanned === 0) return;

    // 2. Группировка по размеру
    const bySize = new Map(); // size -> [FileInfo]
    for (const f of allFiles) {
        if (!bySize.has(f.size)) bySize.set(f.size, []);
        bySize.get(f.size).push(f);
    }

    // 3. Файлы для хеширования
    const filesToHash = [];
    for (const group of bySize.values()) {
        if (group.length > 1) filesToHash.push(...group);
    }

    // 4. Параллельное хеширование
    if (filesToHash.length) {
        await parallelHash(filesToHash);
    }

    // 5. Формирование дубликатов
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
                    f.path,
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

    // 6. Запись CSV и sh
    const csvHeader = ['FileID', 'Path', 'Size', 'Hash', 'CreationTime', 'ModificationTime'];
    const csvContent = [csvHeader, ...csvRows].map(row => row.join(',')).join('\n');
    await fs.writeFile('duplicates_js.csv', csvContent);
    await fs.writeFile('duprm_js.sh', bashLines.join('\n'));
    await fs.chmod('duprm_js.sh', 0o755);
    console.log('Создан duprm_js.sh');

    // 7. Сортированный CSV всех файлов
    const sorted = [...allFiles].sort((a,b) => b.size - a.size);
    const sortRows = sorted.map(f => [
        f.id, f.path, f.size, f.hash || '',
        formatTime(f.birthTime), formatTime(f.modTime)
    ]);
    const sortContent = [csvHeader, ...sortRows].map(row => row.join(',')).join('\n');
    await fs.writeFile('sort_dup_js.csv', sortContent);

    // 8. Статистика
    const elapsed = (Date.now() - startTotal) / 1000;
    console.log(`\nПросмотрено файлов: ${scanned}`);
    console.log(`Найдено дубликатов (файлов для удаления): ${duplicatesCount}`);
    console.log(`Время работы:`);
    console.log(`  - ${elapsed.toFixed(3)} секунд`);
    console.log(`  - ${formatDuration(elapsed)}`);
    console.log(`Отчёты: duplicates_js.csv, sort_dup_js.csv`);
    console.log(`Скрипт удаления: duprm_js.sh`);
}

main().catch(err => {
    console.error(err);
    process.exit(1);
});
