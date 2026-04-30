// find_dups.go
package main

import (
	"crypto/sha256"
	"encoding/csv"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"sync"
	"syscall"
	"time"
)

type FileInfo struct {
	ID      int
	Path    string
	Size    int64
	ModTime time.Time
	Birth   time.Time
	Hash    string
}

func main() {
	if len(os.Args) < 2 {
		fmt.Println("Использование: find_dups <директория1> [директория2 ...]")
		os.Exit(1)
	}
	roots := os.Args[1:]

	startTotal := time.Now()

	// ---------- 1. Сбор всех файлов ----------
	var allFiles []FileInfo
	var scanned int
	var nextID = 1
	var mu sync.Mutex

	for _, root := range roots {
		_ = filepath.WalkDir(root, func(path string, d os.DirEntry, err error) error {
			if err != nil {
				return nil
			}
			if d.IsDir() {
				return nil
			}
			info, err := d.Info()
			if err != nil || !info.Mode().IsRegular() {
				return nil
			}
			size := info.Size()
			modTime := info.ModTime()
			var birthTime time.Time
			if stat, ok := info.Sys().(*syscall.Stat_t); ok {
				birthTime = time.Unix(stat.Birthtimespec.Sec, stat.Birthtimespec.Nsec)
			} else {
				birthTime = modTime
			}
			mu.Lock()
			defer mu.Unlock()
			allFiles = append(allFiles, FileInfo{
				ID:      nextID,
				Path:    path,
				Size:    size,
				ModTime: modTime,
				Birth:   birthTime,
			})
			nextID++
			scanned++
			return nil
		})
	}

	// Группировка по размеру
	bySize := make(map[int64][]*FileInfo)
	for i := range allFiles {
		bySize[allFiles[i].Size] = append(bySize[allFiles[i].Size], &allFiles[i])
	}

	// Собираем файлы для хеширования (те, размер которых встречается более одного раза)
	var filesToHash []*FileInfo
	for _, files := range bySize {
		if len(files) > 1 {
			filesToHash = append(filesToHash, files...)
		}
	}
	totalToHash := len(filesToHash)

	// ---------- 2. Параллельное вычисление хешей ----------
	if totalToHash > 0 {
		fmt.Printf("Параллельное хеширование %d файлов (воркеров: %d)...\n", totalToHash, runtime.NumCPU())
		startHash := time.Now()

		type hashJob struct {
			idx int
			f   *FileInfo
		}
		jobs := make(chan hashJob, len(filesToHash))
		var wg sync.WaitGroup

		// Запуск воркеров
		for w := 0; w < runtime.NumCPU(); w++ {
			wg.Add(1)
			go func() {
				defer wg.Done()
				for job := range jobs {
					hash, err := computeSHA256(job.f.Path)
					if err != nil {
						fmt.Fprintf(os.Stderr, "Ошибка чтения %s: %v\n", job.f.Path, err)
						continue
					}
					job.f.Hash = hash
				}
			}()
		}

		// Отправка заданий
		for idx, f := range filesToHash {
			jobs <- hashJob{idx: idx, f: f}
		}
		close(jobs)
		wg.Wait()

		fmt.Printf("Хеширование завершено за %v\n", time.Since(startHash))
	}

	// ---------- 3. CSV для дубликатов (duplicates_go.csv) ----------
	csvFile, _ := os.Create("duplicates_go.csv")
	defer csvFile.Close()
	csvWriter := csv.NewWriter(csvFile)
	defer csvWriter.Flush()
	csvWriter.Write([]string{"FileID", "Path", "Size", "Hash", "CreationTime", "ModificationTime"})

	shFile, _ := os.Create("duprm_go.sh")
	defer shFile.Close()
	shFile.WriteString("#!/bin/bash\n# Сгенерировано find_dups\nset -e\n\n")

	duplicatesCount := 0
	for size, files := range bySize {
		if len(files) < 2 {
			continue
		}
		hashGroups := make(map[string][]*FileInfo)
		for _, f := range files {
			hashGroups[f.Hash] = append(hashGroups[f.Hash], f)
		}
		for hash, group := range hashGroups {
			if len(group) < 2 {
				continue
			}
			// Записываем все файлы группы в CSV
			for _, dup := range group {
				csvWriter.Write([]string{
					fmt.Sprint(dup.ID),
					dup.Path,
					fmt.Sprint(size),
					hash,
					dup.Birth.Format(time.RFC3339),
					dup.ModTime.Format(time.RFC3339),
				})
			}
			// Добавляем удаление всех, кроме первого (по ID)
			sort.Slice(group, func(i, j int) bool { return group[i].ID < group[j].ID })
			for i, dup := range group {
				if i == 0 {
					continue
				}
				duplicatesCount++
				fmt.Fprintf(shFile, "rm -- %q\n", dup.Path)
			}
		}
	}
	shFile.WriteString("\necho \"Удаление дубликатов завершено.\"\n")
	shFile.Chmod(0755)

	// ---------- 4. Сортированный CSV всех файлов (sort_dup_go.csv) ----------
	sortedFiles := make([]FileInfo, len(allFiles))
	copy(sortedFiles, allFiles)
	sort.Slice(sortedFiles, func(i, j int) bool {
		return sortedFiles[i].Size > sortedFiles[j].Size // по убыванию
	})

	sortFile, _ := os.Create("sort_dup_go.csv")
	defer sortFile.Close()
	sortWriter := csv.NewWriter(sortFile)
	defer sortWriter.Flush()
	sortWriter.Write([]string{"FileID", "Path", "Size", "Hash", "CreationTime", "ModificationTime"})
	for _, f := range sortedFiles {
		sortWriter.Write([]string{
			fmt.Sprint(f.ID),
			f.Path,
			fmt.Sprint(f.Size),
			f.Hash,
			f.Birth.Format(time.RFC3339),
			f.ModTime.Format(time.RFC3339),
		})
	}

	// ---------- 5. Вывод статистики ----------
	elapsed := time.Since(startTotal)
	fmt.Printf("\nПросмотрено файлов: %d\n", scanned)
	fmt.Printf("Найдено дубликатов (файлов для удаления): %d\n", duplicatesCount)
	fmt.Printf("Время работы:\n")
	fmt.Printf("  - %.3f секунд\n", elapsed.Seconds())
	fmt.Printf("  - %s\n", formatDuration(elapsed))
	fmt.Println("Отчёты: duplicates_go.csv, sort_dup_go.csv")
	fmt.Println("Скрипт удаления: duprm_go.sh")
}

// computeSHA256 вычисляет хеш файла
func computeSHA256(path string) (string, error) {
	f, err := os.Open(path)
	if err != nil {
		return "", err
	}
	defer f.Close()
	h := sha256.New()
	if _, err := io.Copy(h, f); err != nil {
		return "", err
	}
	return fmt.Sprintf("%x", h.Sum(nil)), nil
}

// formatDuration форматирует время в "ч:м:с.мс"
func formatDuration(d time.Duration) string {
	ms := d.Milliseconds()
	s := ms / 1000
	ms %= 1000
	m := s / 60
	s %= 60
	h := m / 60
	m %= 60
	return fmt.Sprintf("%02d:%02d:%02d.%03d", h, m, s, ms)
}
