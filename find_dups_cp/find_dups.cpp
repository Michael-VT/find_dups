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
#include <atomic>
#include <sys/stat.h>
#include <openssl/evp.h>

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

    EVP_MD_CTX* ctx = EVP_MD_CTX_new();
    EVP_DigestInit_ex(ctx, EVP_sha256(), nullptr);

    char buffer[BUFFER_SIZE];
    while (file.read(buffer, sizeof(buffer))) {
        EVP_DigestUpdate(ctx, buffer, sizeof(buffer));
    }
    EVP_DigestUpdate(ctx, buffer, file.gcount());

    unsigned char hash[EVP_MAX_MD_SIZE];
    unsigned int hash_len = 0;
    EVP_DigestFinal_ex(ctx, hash, &hash_len);
    EVP_MD_CTX_free(ctx);

    std::ostringstream oss;
    for (unsigned int i = 0; i < hash_len; i++)
        oss << std::hex << std::setw(2) << std::setfill('0') << (int)hash[i];
    return oss.str();
}

std::vector<FileInfo> collect_files(const std::vector<Path>& roots, uint64_t& scanned) {
    std::vector<FileInfo> files;
    uint64_t next_id = 1;

    for (const auto& root : roots) {
        try {
            for (auto& entry : fs::recursive_directory_iterator(root,
                            fs::directory_options::skip_permission_denied)) {
                if (entry.is_symlink()) continue;
                if (!entry.is_regular_file()) continue;

                auto path = entry.path();
                auto size = entry.file_size();
                if (size == 0) continue; // skip zero-byte files

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
            std::cerr << "Warning: " << e.what() << std::endl;
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

std::string csv_escape(const std::string& s) {
    if (s.find(',') != std::string::npos || s.find('"') != std::string::npos || s.find('\n') != std::string::npos) {
        std::string escaped = "\"";
        for (char c : s) {
            if (c == '"') escaped += "\"\"";
            else escaped += c;
        }
        escaped += "\"";
        return escaped;
    }
    return s;
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

std::string to_lower(std::string s) {
    std::transform(s.begin(), s.end(), s.begin(), ::tolower);
    return s;
}

std::string get_category(const std::string& ext) {
    if (ext == ".c" || ext == ".h" || ext == ".cpp" || ext == ".hpp" || ext == ".cc" || ext == ".cxx" ||
        ext == ".m" || ext == ".mm" || ext == ".s" || ext == ".S" || ext == ".java" || ext == ".kt" ||
        ext == ".py" || ext == ".js" || ext == ".ts" || ext == ".rs" || ext == ".go" || ext == ".rb" ||
        ext == ".swift" || ext == ".sh") return "source";
    if (ext == ".hex" || ext == ".bin" || ext == ".elf" || ext == ".dfu" || ext == ".flash" || ext == ".map") return "firmware";
    if (ext == ".uvprojx" || ext == ".uvoptx" || ext == ".ewp" || ext == ".eww" || ext == ".ewt" ||
        ext == ".cproject" || ext == ".project" || ext == ".mxproject" || ext == ".ioc") return "ide";
    if (ext == ".yaml" || ext == ".yml" || ext == ".cmake" || ext == ".json" || ext == ".xml" ||
        ext == ".conf" || ext == ".cfg" || ext == ".ini" || ext == ".toml" || ext == ".properties") return "config";
    if (ext == ".pdf" || ext == ".md" || ext == ".txt" || ext == ".html" || ext == ".htm" ||
        ext == ".rst" || ext == ".chm" || ext == ".doc" || ext == ".docx" || ext == ".rtf") return "docs";
    if (ext == ".png" || ext == ".jpg" || ext == ".jpeg" || ext == ".gif" || ext == ".webp" ||
        ext == ".svg" || ext == ".bmp" || ext == ".ico" || ext == ".tiff" || ext == ".tif") return "image";
    if (ext == ".exe" || ext == ".dll" || ext == ".so" || ext == ".dylib" || ext == ".o" || ext == ".a" ||
        ext == ".lib" || ext == ".obj" || ext == ".gch" || ext == ".pch") return "binary";
    if (ext == ".zip" || ext == ".7z" || ext == ".tar" || ext == ".gz" || ext == ".bz2" ||
        ext == ".xz" || ext == ".rar" || ext == ".tgz") return "archive";
    if (ext == ".mp4" || ext == ".wav" || ext == ".avi" || ext == ".mp3" || ext == ".ogg" ||
        ext == ".flac" || ext == ".mov" || ext == ".wmv") return "media";
    if (ext == ".ttf" || ext == ".otf" || ext == ".woff" || ext == ".woff2") return "font";
    if (ext == ".csv" || ext == ".dts" || ext == ".dtsi" || ext == ".overlay" || ext == ".ld" ||
        ext == ".icf" || ext == ".srec") return "data";
    return "other";
}

void generate_analytics(const std::vector<FileInfo>& all_files,
                        const std::unordered_map<uint64_t, std::vector<FileInfo*>>& by_size,
                        double elapsed) {
    struct CatStats { uint64_t count = 0, total_bytes = 0, dup_count = 0, dup_bytes = 0; };
    struct ExtStats { uint64_t count = 0, total_bytes = 0, dup_count = 0, dup_bytes = 0; };

    std::unordered_map<std::string, CatStats> cat_map;
    std::unordered_map<std::string, ExtStats> ext_map;
    uint64_t total_size = 0, dup_files = 0, dup_bytes = 0;
    int size_bins[6] = {0, 0, 0, 0, 0, 0}; // 0b, <1k, 1k-100k, 100k-1m, 1m-100m, >100m

    for (const auto& f : all_files) {
        std::string ext_s = to_lower(f.path.extension().string());
        std::string cat = get_category(ext_s);
        cat_map[cat].count++;
        cat_map[cat].total_bytes += f.size;
        ext_map[ext_s].count++;
        ext_map[ext_s].total_bytes += f.size;
        total_size += f.size;
        if (f.size == 0) size_bins[0]++;
        else if (f.size < 1024) size_bins[1]++;
        else if (f.size < 102400) size_bins[2]++;
        else if (f.size < 1048576) size_bins[3]++;
        else if (f.size < 104857600) size_bins[4]++;
        else size_bins[5]++;
    }

    // Identify duplicates
    for (const auto& kv : by_size) {
        auto& group = kv.second;
        if (group.size() < 2) continue;
        std::unordered_map<Hash, std::vector<FileInfo*>> hg;
        for (auto* f : group) { if (!f->hash.empty()) hg[f->hash].push_back(f); }
        for (auto& h : hg) {
            if (h.second.size() < 2) continue;
            for (size_t i = 1; i < h.second.size(); ++i) {
                auto* f = h.second[i];
                std::string ext_s = f->path.extension().string();
                std::string cat = get_category(ext_s);
                cat_map[cat].dup_count++;
                cat_map[cat].dup_bytes += f->size;
                ext_map[ext_s].dup_count++;
                ext_map[ext_s].dup_bytes += f->size;
                dup_files++;
                dup_bytes += f->size;
            }
        }
    }

    uint64_t recoverable = dup_bytes;

    // Write JSON
    std::ofstream jf("analytics_cpp.json");
    jf << "{\n";
    jf << "  \"summary\": {";
    jf << "\"total_files\": " << all_files.size() << ", ";
    jf << "\"total_size_bytes\": " << total_size << ", ";
    jf << "\"duplicate_files\": " << dup_files << ", ";
    jf << "\"duplicate_size_bytes\": " << dup_bytes << ", ";
    jf << "\"recoverable_bytes\": " << recoverable << ", ";
    jf << "\"scan_duration_seconds\": " << std::fixed << std::setprecision(3) << elapsed << "},\n";

    jf << "  \"by_category\": {\n";
    bool first_cat = true;
    for (auto& kv : cat_map) {
        if (!first_cat) jf << ",\n";
        first_cat = false;
        jf << "    \"" << kv.first << "\": {";
        jf << "\"count\": " << kv.second.count << ", ";
        jf << "\"total_bytes\": " << kv.second.total_bytes << ", ";
        jf << "\"duplicate_count\": " << kv.second.dup_count << ", ";
        jf << "\"duplicate_bytes\": " << kv.second.dup_bytes << ", ";
        jf << "\"extensions\": {";
        bool first_ext = true;
        for (auto& ekv : ext_map) {
            if (get_category(ekv.first) != kv.first) continue;
            if (!first_ext) jf << ", ";
            first_ext = false;
            jf << "\"" << ekv.first << "\": " << ekv.second.count;
        }
        jf << "}}";
    }
    jf << "\n  },\n";

    jf << "  \"by_extension\": {\n";
    bool first_e = true;
    for (auto& kv : ext_map) {
        if (!first_e) jf << ",\n";
        first_e = false;
        jf << "    \"" << kv.first << "\": {";
        jf << "\"count\": " << kv.second.count << ", ";
        jf << "\"total_bytes\": " << kv.second.total_bytes << ", ";
        jf << "\"duplicate_count\": " << kv.second.dup_count << ", ";
        jf << "\"duplicate_bytes\": " << kv.second.dup_bytes << "}";
    }
    jf << "\n  },\n";

    jf << "  \"size_distribution\": {";
    jf << "\"0_bytes\": " << size_bins[0] << ", ";
    jf << "\"under_1kb\": " << size_bins[1] << ", ";
    jf << "\"1kb_100kb\": " << size_bins[2] << ", ";
    jf << "\"100kb_1mb\": " << size_bins[3] << ", ";
    jf << "\"1mb_100mb\": " << size_bins[4] << ", ";
    jf << "\"over_100mb\": " << size_bins[5] << "}\n";
    jf << "}\n";

    // Print summary
    std::cout << "\n--- File Type Analytics ---\n";
    for (auto& kv : cat_map) {
        std::cout << "  " << std::setw(10) << std::left << kv.first
                  << ": " << kv.second.count << " files, "
                  << kv.second.dup_count << " duplicates\n";
    }
    std::cout << "Analytics written to analytics_cpp.json\n";
}

int main(int argc, char* argv[]) {
    if (argc < 2) {
        std::cerr << "Usage: find_dups_cpp <dir1> [<dir2> ...]" << std::endl;
        return 1;
    }

    std::vector<Path> roots;
    for (int i = 1; i < argc; ++i) roots.emplace_back(argv[i]);

    auto start_total = std::chrono::steady_clock::now();

    std::cout << "Collecting files... " << std::flush;
    uint64_t scanned = 0;
    auto all_files = collect_files(roots, scanned);
    std::cout << "found " << scanned << " files" << std::endl;
    if (scanned == 0) return 0;

    auto by_size = group_by_size(all_files);

    // --- Collect files to hash (size groups with >1 file) ---
    std::vector<FileInfo*> files_to_hash;
    for (auto& kv : by_size) {
        if (kv.second.size() > 1) {
            files_to_hash.insert(files_to_hash.end(), kv.second.begin(), kv.second.end());
        }
    }

    if (!files_to_hash.empty()) {
        std::cout << "Parallel hashing " << files_to_hash.size()
                  << " files (workers: " << NUM_WORKERS << ")..." << std::endl;
        auto start_hash = std::chrono::steady_clock::now();

        auto hash_worker = [](std::vector<FileInfo*>* chunk) {
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
            futures.push_back(std::async(std::launch::async, hash_worker, &chunk));
        }
        for (auto& f : futures) f.get();

        auto end_hash = std::chrono::steady_clock::now();
        std::chrono::duration<double> hash_elapsed = end_hash - start_hash;
        std::cout << "Hashing completed in " << hash_elapsed.count() << " seconds" << std::endl;
    }

    // ----- Duplicate detection -----
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
                csv_file << csv_escape(row[i]);
            }
            csv_file << '\n';
        }
    } else {
        std::cerr << "Error creating duplicates_cpp.csv\n";
        return 1;
    }

    std::ofstream sh_file("duprm_cpp.sh");
    if (sh_file) {
        for (auto& line : bash_lines) sh_file << line << '\n';
        sh_file.close();
        fs::permissions("duprm_cpp.sh", fs::perms::owner_exec | fs::perms::group_exec | fs::perms::others_exec,
                        fs::perm_options::add);
    } else {
        std::cerr << "Error creating duprm_cpp.sh\n";
    }

    std::vector<FileInfo> sorted = all_files;
    std::sort(sorted.begin(), sorted.end(), [](auto& a, auto& b) { return a.size > b.size; });
    std::ofstream sort_file("sort_dup_cpp.csv");
    if (sort_file) {
        sort_file << "FileID,Path,Size,Hash,CreationTime,ModificationTime\n";
        for (auto& f : sorted) {
            sort_file << csv_escape(std::to_string(f.id)) << ','
                      << csv_escape(f.path.string()) << ','
                      << csv_escape(std::to_string(f.size)) << ','
                      << csv_escape(f.hash) << ','
                      << csv_escape(format_time(f.birth_time)) << ','
                      << csv_escape(format_time(f.mod_time)) << '\n';
        }
    } else {
        std::cerr << "Error creating sort_dup_cpp.csv\n";
    }

    auto end_total = std::chrono::steady_clock::now();
    Duration elapsed = end_total - start_total;
    double sec = elapsed.count();

    generate_analytics(all_files, by_size, sec);

    std::cout << "\nFiles scanned: " << scanned << std::endl;
    std::cout << "Duplicates found (files to delete): " << duplicates_count << std::endl;
    std::cout << "Runtime:\n";
    std::cout << "  - " << std::fixed << std::setprecision(3) << sec << " seconds\n";
    std::cout << "  - " << format_duration(sec) << std::endl;
    std::cout << "Reports: duplicates_cpp.csv, sort_dup_cpp.csv\n";
    std::cout << "Delete script: duprm_cpp.sh\n";

    return 0;
}
