#include <iostream>
#include <fstream>
#include <vector>
#include <string>
#include <filesystem>
#include <unordered_map>
#include <algorithm>
#include <chrono>
#include <iomanip>
#include <sstream>
#include <thread>
#include <future>
#include <mutex>
#include <queue>
#include <condition_variable>
#include <sys/stat.h>       // для stat, st_birthtime
#include <openssl/sha.h>

namespace fs = std::filesystem;

using Path = fs::path;
using Hash = std::string;
using TimePoint = std::chrono::system_clock::time_point;
using Duration = std::chrono::duration<double>;

struct FileInfo {
    uint64_t id;
    Path path;
    uint64_t size;
    TimePoint mod_time;
    TimePoint birth_time;
    Hash hash;
};

const size_t BUFFER_SIZE = 65536;
const unsigned int NUM_WORKERS = std::thread::hardware_concurrency();

std::string format_time(TimePoint tp) {
    auto tt = std::chrono::system_clock::to_time_t(tp);
    std::tm tm = *std::gmtime(&tt);
    std::ostringstream oss;
    oss << std::put_time(&tm, "%Y-%m-%dT%H:%M:%S") << "Z";
    return oss.str();
}

Hash compute_sha256(const Path& path) {
    std::ifstream file(path, std::ios::binary);
    if (!file.is_open()) return "";
    
    SHA256_CTX ctx;
    SHA256_Init(&ctx);
    
    char buffer[BUFFER_SIZE];
    while (file.read(buffer, sizeof(buffer))) {
        SHA256_Update(&ctx, buffer, sizeof(buffer));
    }
    SHA256_Update(&ctx, buffer, file.gcount());
    
    unsigned char hash[SHA256_DIGEST_LENGTH];
    SHA256_Final(hash, &ctx);
    
    std::ostringstream oss;
    for (unsigned char c : hash) oss << std::hex << std::setw(2) << std::setfill('0') << (int)c;
    return oss.str();
}

//std::vector<FileInfo> collect_files(const std::vector<Path>& roots, uint64_t& scanned) {
//    std::vector<FileInfo> files;
//    uint64_t next_id = 1;
//    
//    for (const auto& root : roots) {
//        try {
//            for (auto& entry : fs::recursive_directory_iterator(root,
//                            fs::directory_options::skip_permission_denied)) {
//                if (!entry.is_regular_file()) continue;
//                auto st = entry.status();
//                if (!fs::is_regular_file(st)) continue;
//                
//                auto path = entry.path();
//                auto size = entry.file_size();
//                auto mod_time = entry.last_write_time();
//                auto sys_mtime = std::chrono::time_point_cast<std::chrono::system_clock::duration>(
//                                    mod_time - fs::file_time_type::clock::now() + 
//                                    std::chrono::system_clock::now());
//                
//                struct stat stat_buf;
//                if (stat(path.c_str(), &stat_buf) == 0) {
//                    auto birth = std::chrono::system_clock::from_time_t(stat_buf.st_birthtime);
//                    files.push_back({next_id++, path, size, sys_mtime, birth, ""});
//                } else {
//                    files.push_back({next_id++, path, size, sys_mtime, sys_mtime, ""});
//                }
//                scanned++;
//            }
//        } catch (const fs::filesystem_error& e) {
//            std::cerr << "Предупреждение: " << e.what() << std::endl;
//        }
//    }
//    return files;
//}

std::vector<FileInfo> collect_files(const std::vector<Path>& roots, uint64_t& scanned) {
    std::vector<FileInfo> files;
    uint64_t next_id = 1;
    
    for (const auto& root : roots) {
        try {
            for (auto& entry : fs::recursive_directory_iterator(root,
                            fs::directory_options::skip_permission_denied)) {
                // Пропускаем символические ссылки
                if (entry.is_symlink()) continue;
                if (!entry.is_regular_file()) continue;
                
                auto path = entry.path();
                auto size = entry.file_size();
                auto mod_time = entry.last_write_time();
                auto sys_mtime = std::chrono::time_point_cast<std::chrono::system_clock::duration>(
                                    mod_time - fs::file_time_type::clock::now() + 
                                    std::chrono::system_clock::now());
                
                struct stat stat_buf;
                if (stat(path.c_str(), &stat_buf) == 0) {
                    auto birth = std::chrono::system_clock::from_time_t(stat_buf.st_birthtime);
                    files.push_back({next_id++, path, size, sys_mtime, birth, ""});
                } else {
                    files.push_back({next_id++, path, size, sys_mtime, sys_mtime, ""});
                }
                scanned++;
            }
        } catch (const fs::filesystem_error& e) {
            std::cerr << "Предупреждение: " << e.what() << std::endl;
        }
    }
    return files;
}

std::unordered_map<uint64_t, std::vector<FileInfo*>> group_by_size(std::vector<FileInfo>& files) {
    std::unordered_map<uint64_t, std::vector<FileInfo*>> groups;
    for (auto& f : files) {
        groups[f.size].push_back(&f);
    }
    return groups;
}

void parallel_hash(std::vector<FileInfo*>& files_to_hash) {
    if (files_to_hash.empty()) return;
    
    std::cout << "Параллельное хеширование " << files_to_hash.size() << " файлов (воркеров: " << NUM_WORKERS << ")..." << std::endl;
    auto start = std::chrono::steady_clock::now();
    
    auto worker = [](std::vector<FileInfo*>* chunk) {
        for (auto* f : *chunk) {
            f->hash = compute_sha256(f->path);
        }
    };
    
    size_t chunk_size = (files_to_hash.size() + NUM_WORKERS - 1) / NUM_WORKERS;
    std::vector<std::vector<FileInfo*>> chunks;
    for (size_t i = 0; i < files_to_hash.size(); i += chunk_size) {
        chunks.emplace_back(files_to_hash.begin() + i,
                            files_to_hash.begin() + std::min(i + chunk_size, files_to_hash.size()));
    }
    
    std::vector<std::future<void>> futures;
    for (auto& chunk : chunks) {
        futures.push_back(std::async(std::launch::async, worker, &chunk));
    }
    for (auto& f : futures) f.get();
    
    auto end = std::chrono::steady_clock::now();
    std::chrono::duration<double> elapsed = end - start;
    std::cout << "Хеширование завершено за " << elapsed.count() << " секунд" << std::endl;
}

std::string format_duration(double seconds) {
    int64_t ms_total = static_cast<int64_t>(seconds * 1000);
    int64_t ms = ms_total % 1000;
    int64_t s = ms_total / 1000;
    int64_t m = s / 60;
    s %= 60;
    int64_t h = m / 60;
    m %= 60;
    std::ostringstream oss;
    oss << std::setw(2) << std::setfill('0') << h << ":"
        << std::setw(2) << m << ":"
        << std::setw(2) << s << "."
        << std::setw(3) << ms;
    return oss.str();
}

int main(int argc, char* argv[]) {
    if (argc < 2) {
        std::cerr << "Использование: find_dups_cpp <директория1> [<директория2> ...]" << std::endl;
        return 1;
    }
    
    std::vector<Path> roots;
    for (int i = 1; i < argc; ++i) roots.emplace_back(argv[i]);
    
    auto start_total = std::chrono::steady_clock::now();
    
    std::cout << "Сбор файлов... " << std::flush;
    uint64_t scanned = 0;
    auto all_files = collect_files(roots, scanned);
    std::cout << "найдено " << scanned << " файлов" << std::endl;
    
    auto by_size = group_by_size(all_files);
    
    std::vector<FileInfo*> files_to_hash;
    for (auto& kv : by_size) {
        if (kv.second.size() > 1) {
            files_to_hash.insert(files_to_hash.end(), kv.second.begin(), kv.second.end());
        }
    }
    if (!files_to_hash.empty()) {
        parallel_hash(files_to_hash);
    }
    
    std::vector<std::vector<std::string>> csv_dups;
    std::vector<std::string> bash_lines = {"#!/bin/bash", "# Generated by find_dups_cpp", "set -e", ""};
    uint64_t duplicates_count = 0;
    
    for (auto& kv : by_size) {
        auto& group = kv.second;
        if (group.size() < 2) continue;
        
        std::unordered_map<Hash, std::vector<FileInfo*>> hash_groups;
        for (auto* f : group) {
            if (!f->hash.empty()) hash_groups[f->hash].push_back(f);
        }
        for (auto& hg : hash_groups) {
            auto& same = hg.second;
            if (same.size() < 2) continue;
            std::sort(same.begin(), same.end(), [](auto a, auto b) { return a->id < b->id; });
            for (auto* f : same) {
                csv_dups.push_back({
                    std::to_string(f->id),
                    f->path.string(),
                    std::to_string(f->size),
                    f->hash,
                    format_time(f->birth_time),
                    format_time(f->mod_time)
                });
            }
            for (size_t i = 1; i < same.size(); ++i) {
                duplicates_count++;
                std::string escaped = same[i]->path.string();
                size_t pos = 0;
                while ((pos = escaped.find('\'', pos)) != std::string::npos) {
                    escaped.replace(pos, 1, "'\\''");
                    pos += 4;
                }
                bash_lines.push_back("rm -- '" + escaped + "'");
            }
        }
    }
    bash_lines.push_back("");
    bash_lines.push_back("echo \"Deletion complete.\"");
    
    std::ofstream csv_file("duplicates_cpp.csv");
    if (csv_file) {
        csv_file << "FileID,Path,Size,Hash,CreationTime,ModificationTime\n";
        for (auto& row : csv_dups) {
            for (size_t i = 0; i < row.size(); ++i) {
                if (i) csv_file << ',';
                csv_file << row[i];
            }
            csv_file << '\n';
        }
    } else {
        std::cerr << "Ошибка создания duplicates_cpp.csv\n";
        return 1;
    }
    
    std::ofstream sh_file("duprm_cpp.sh");
    if (sh_file) {
        for (auto& line : bash_lines) sh_file << line << '\n';
        sh_file.close();
        fs::permissions("duprm_cpp.sh", fs::perms::owner_exec | fs::perms::group_exec | fs::perms::others_exec,
                        fs::perm_options::add);
    } else {
        std::cerr << "Ошибка создания duprm_cpp.sh\n";
    }
    
    std::vector<FileInfo> sorted = all_files;
    std::sort(sorted.begin(), sorted.end(), [](auto& a, auto& b) { return a.size > b.size; });
    std::ofstream sort_file("sort_dup_cpp.csv");
    if (sort_file) {
        sort_file << "FileID,Path,Size,Hash,CreationTime,ModificationTime\n";
        for (auto& f : sorted) {
            sort_file << f.id << ','
                      << f.path.string() << ','
                      << f.size << ','
                      << f.hash << ','
                      << format_time(f.birth_time) << ','
                      << format_time(f.mod_time) << '\n';
        }
    } else {
        std::cerr << "Ошибка создания sort_dup_cpp.csv\n";
    }
    
    auto end_total = std::chrono::steady_clock::now();
    Duration elapsed = end_total - start_total;
    double sec = elapsed.count();
    std::cout << "\nПросмотрено файлов: " << scanned << std::endl;
    std::cout << "Найдено дубликатов (файлов для удаления): " << duplicates_count << std::endl;
    std::cout << "Время работы:\n";
    std::cout << "  - " << std::fixed << std::setprecision(3) << sec << " секунд\n";
    std::cout << "  - " << format_duration(sec) << std::endl;
    std::cout << "Отчёты: duplicates_cpp.csv, sort_dup_cpp.csv\n";
    std::cout << "Скрипт удаления: duprm_cpp.sh\n";
    
    return 0;
}

