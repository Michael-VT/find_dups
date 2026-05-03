// find_dups.go
package main

import (
	"crypto/sha256"
	"encoding/csv"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strings"
	"sync"
	"sync/atomic"
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

var categoryMap = map[string]string{
	".c": "source", ".h": "source", ".cpp": "source", ".hpp": "source",
	".cc": "source", ".cxx": "source", ".m": "source", ".mm": "source",
	".s": "source", ".S": "source", ".java": "source", ".kt": "source",
	".py": "source", ".js": "source", ".ts": "source", ".rs": "source",
	".go": "source", ".rb": "source", ".swift": "source", ".sh": "source",

	".hex": "firmware", ".bin": "firmware", ".elf": "firmware", ".dfu": "firmware",
	".flash": "firmware", ".map": "firmware",

	".uvprojx": "ide", ".uvoptx": "ide", ".ewp": "ide", ".eww": "ide",
	".ewt": "ide", ".cproject": "ide", ".project": "ide", ".mxproject": "ide", ".ioc": "ide",

	".yaml": "config", ".yml": "config", ".cmake": "config", ".json": "config",
	".xml": "config", ".conf": "config", ".cfg": "config", ".ini": "config",
	".toml": "config", ".properties": "config",

	".pdf": "docs", ".md": "docs", ".txt": "docs", ".html": "docs",
	".htm": "docs", ".rst": "docs", ".chm": "docs", ".doc": "docs",
	".docx": "docs", ".rtf": "docs",

	".png": "image", ".jpg": "image", ".jpeg": "image", ".gif": "image",
	".webp": "image", ".svg": "image", ".bmp": "image", ".ico": "image",
	".tiff": "image", ".tif": "image",

	".exe": "binary", ".dll": "binary", ".so": "binary", ".dylib": "binary",
	".o": "binary", ".a": "binary", ".lib": "binary", ".obj": "binary",
	".gch": "binary", ".pch": "binary",

	".zip": "archive", ".7z": "archive", ".tar": "archive", ".gz": "archive",
	".bz2": "archive", ".xz": "archive", ".rar": "archive", ".tgz": "archive",

	".mp4": "media", ".wav": "media", ".avi": "media", ".mp3": "media",
	".ogg": "media", ".flac": "media", ".mov": "media", ".wmv": "media",

	".ttf": "font", ".otf": "font", ".woff": "font", ".woff2": "font",

	".csv": "data", ".dts": "data", ".dtsi": "data", ".overlay": "data",
	".ld": "data", ".icf": "data", ".srec": "data",
}

// ProgressTracker tracks and displays file-based progress every 1 second with spinner animation
type ProgressTracker struct {
	total      int64
	processed  int64
	itemName   string
	startTime  time.Time
	stopChan   chan struct{}
}

func NewProgressTracker(total int, itemName string) *ProgressTracker {
	return &ProgressTracker{
		total:     int64(total),
		processed: 0,
		itemName:  itemName,
		startTime: time.Now(),
		stopChan:  make(chan struct{}),
	}
}

func (pt *ProgressTracker) Start() {
	ticker := time.NewTicker(1 * time.Second)
	spinners := []string{"|", "/", "-", "\\"}
	spinIdx := 0
	go func() {
		for {
			select {
			case <-ticker.C:
				current := atomic.LoadInt64(&pt.processed)
				if current >= pt.total {
					return
				}
				spinChar := spinners[spinIdx]
				spinIdx = (spinIdx + 1) % len(spinners)
				fmt.Printf("\n\rHashing: %d/%d files %s", current, pt.total, spinChar)
			case <-pt.stopChan:
				return
			}
		}
	}()
}

func (pt *ProgressTracker) Increment() {
	atomic.AddInt64(&pt.processed, 1)
}

func (pt *ProgressTracker) Stop() {
    close(pt.stopChan)
    fmt.Printf("\rHashing: %d/%d files \u2588\n", pt.total, pt.total)
}

func getCategory(ext string) string {
	if cat, ok := categoryMap[ext]; ok {
		return cat
	}
	return "other"
}

func generateAnalytics(allFiles []FileInfo, bySize map[int64][]*FileInfo, elapsed time.Duration) {
	// Identify duplicate file IDs
	dupSet := make(map[int]bool)
	for _, files := range bySize {
		if len(files) < 2 {
			continue
		}
		hashGroups := make(map[string][]*FileInfo)
		for _, f := range files {
			if f.Hash != "" {
				hashGroups[f.Hash] = append(hashGroups[f.Hash], f)
			}
		}
		for _, group := range hashGroups {
			if len(group) < 2 {
				continue
			}
			sort.Slice(group, func(i, j int) bool { return group[i].ID < group[j].ID })
			for i := 1; i < len(group); i++ {
				dupSet[group[i].ID] = true
			}
		}
	}

	// Accumulate stats
	type catStats struct {
		count         int
		totalBytes    int64
		dupCount      int
		dupBytes      int64
		extensions    map[string]int
	}
	type extStats struct {
		count      int
		totalBytes int64
		dupCount   int
		dupBytes   int64
	}

	catMap := make(map[string]*catStats)
	extMap := make(map[string]*extStats)

	var totalSize int64
	var dupSize int64
	sizeDist := map[string]int{
		"0_bytes":   0,
		"under_1kb": 0,
		"1kb_100kb": 0,
		"100kb_1mb": 0,
		"1mb_100mb": 0,
		"over_100mb": 0,
	}

	for i := range allFiles {
		f := &allFiles[i]
		ext := strings.ToLower(filepath.Ext(f.Path))
		cat := getCategory(ext)
		isDup := dupSet[f.ID]

		totalSize += f.Size
		if isDup {
			dupSize += f.Size
		}

		// Category stats
		cs, ok := catMap[cat]
		if !ok {
			cs = &catStats{extensions: make(map[string]int)}
			catMap[cat] = cs
		}
		cs.count++
		cs.totalBytes += f.Size
		if isDup {
			cs.dupCount++
			cs.dupBytes += f.Size
		}
		if ext != "" {
			cs.extensions[ext]++
		}

		// Extension stats
		es, ok := extMap[ext]
		if !ok {
			es = &extStats{}
			extMap[ext] = es
		}
		es.count++
		es.totalBytes += f.Size
		if isDup {
			es.dupCount++
			es.dupBytes += f.Size
		}

		// Size distribution
		sz := f.Size
		switch {
		case sz == 0:
			sizeDist["0_bytes"]++
		case sz < 1024:
			sizeDist["under_1kb"]++
		case sz < 100*1024:
			sizeDist["1kb_100kb"]++
		case sz < 1024*1024:
			sizeDist["100kb_1mb"]++
		case sz < 100*1024*1024:
			sizeDist["1mb_100mb"]++
		default:
			sizeDist["over_100mb"]++
		}
	}

	// Build JSON structure
	byCategory := make(map[string]interface{})
	for cat, cs := range catMap {
		exts := make(map[string]int)
		for e, n := range cs.extensions {
			exts[e] = n
		}
		byCategory[cat] = map[string]interface{}{
			"count":          cs.count,
			"total_bytes":    cs.totalBytes,
			"duplicate_count": cs.dupCount,
			"duplicate_bytes": cs.dupBytes,
			"extensions":     exts,
		}
	}

	byExtension := make(map[string]interface{})
	for ext, es := range extMap {
		byExtension[ext] = map[string]interface{}{
			"count":           es.count,
			"total_bytes":     es.totalBytes,
			"duplicate_count": es.dupCount,
			"duplicate_bytes": es.dupBytes,
		}
	}

	result := map[string]interface{}{
		"summary": map[string]interface{}{
			"total_files":          len(allFiles),
			"total_size_bytes":     totalSize,
			"duplicate_files":      len(dupSet),
			"duplicate_size_bytes": dupSize,
			"recoverable_bytes":    dupSize,
			"scan_duration_seconds": elapsed.Seconds(),
		},
		"by_category":      byCategory,
		"by_extension":     byExtension,
		"size_distribution": sizeDist,
	}

	// Write JSON
	jsonData, err := json.MarshalIndent(result, "", "  ")
	if err != nil {
		fmt.Fprintf(os.Stderr, "Analytics JSON error: %v\n", err)
		return
	}
	if err := os.WriteFile("analytics_go.json", jsonData, 0644); err != nil {
		fmt.Fprintf(os.Stderr, "Analytics write error: %v\n", err)
		return
	}

	// Human-readable summary
	fmt.Println("\n--- File Type Analytics ---")
	for _, cat := range sortedKeys(byCategory) {
		entry := byCategory[cat].(map[string]interface{})
		fmt.Printf("  %-12s %5d files, %d duplicates\n",
			cat,
			entry["count"].(int),
			entry["duplicate_count"].(int),
		)
	}
	fmt.Println("Analytics written to analytics_go.json")
}

func humanBytes(b int64) string {
	const (
		kiB = 1024
		miB = 1024 * kiB
		giB = 1024 * miB
	)
	switch {
	case b >= giB:
		return fmt.Sprintf("%.2f GiB", float64(b)/float64(giB))
	case b >= miB:
		return fmt.Sprintf("%.2f MiB", float64(b)/float64(miB))
	case b >= kiB:
		return fmt.Sprintf("%.2f KiB", float64(b)/float64(kiB))
	default:
		return fmt.Sprintf("%d B", b)
	}
}

func formatSize(bytes int64) string {
	const (
		KB = 1024
		MB = 1024 * KB
		GB = 1024 * MB
	)
	switch {
	case bytes >= GB:
		return fmt.Sprintf("%.1f GB", float64(bytes)/float64(GB))
	case bytes >= MB:
		return fmt.Sprintf("%.1f MB", float64(bytes)/float64(MB))
	case bytes >= KB:
		return fmt.Sprintf("%.1f KB", float64(bytes)/float64(KB))
	default:
		return fmt.Sprintf("%d B", bytes)
	}
}
func sortedKeys(m map[string]interface{}) []string {
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	return keys
}

func main() {
	if len(os.Args) < 2 {
		fmt.Println("Usage: find_dups <dir1> [<dir2> ...]")
		os.Exit(1)
	}
	roots := os.Args[1:]

	startTotal := time.Now()

// ----- 1. Collect regular files (no symlinks) -----
	var allFiles []FileInfo
	var scanned int
	var totalSize int64
	var nextID = 1
	
	// Progress update struct
	type progressUpdate struct {
		count int
		size  int64
	}
	
	// Start file collection progress indicator
	progressChan := make(chan progressUpdate, 1)
	stopChan := make(chan struct{})
	go func() {
		ticker := time.NewTicker(5 * time.Second)
		defer ticker.Stop()
		lastCount := 0
		lastSize := int64(0)
		for {
			select {
			case update := <-progressChan:
				lastCount = update.count
				lastSize = update.size
			case <-ticker.C:
				if lastCount > 0 {
					fmt.Printf("\rCollecting files... %d files, %s scanned...", lastCount, formatSize(lastSize))
				}
			case <-stopChan:
				return
			}
		}
	}()

	for _, root := range roots {
		err := filepath.WalkDir(root, func(path string, d os.DirEntry, err error) error {
			if err != nil {
				return nil // ignore access errors
			}
			if d.IsDir() {
				return nil
			}
			info, err := d.Info()
			if err != nil {
				return nil
			}
			if !info.Mode().IsRegular() {
				return nil
			}
			size := info.Size()
			if size == 0 {
				return nil // skip zero-byte files
			}
			modTime := info.ModTime()
			var birthTime time.Time
			if stat, ok := info.Sys().(*syscall.Stat_t); ok {
				birthTime = time.Unix(stat.Birthtimespec.Sec, stat.Birthtimespec.Nsec)
			} else {
				birthTime = modTime
			}
			scanned++
			totalSize += size
			progressChan <- progressUpdate{count: scanned, size: totalSize}
			allFiles = append(allFiles, FileInfo{
				ID:      nextID,
				Path:    path,
				Size:    size,
				ModTime: modTime,
				Birth:   birthTime,
			})
			nextID++
			return nil
		})
		if err != nil {
			fmt.Fprintf(os.Stderr, "Walk error %s: %v\n", root, err)
		}
	}
	
	close(stopChan)
	fmt.Printf("\rCollecting files... found %d files, %s\n", scanned, formatSize(totalSize))
	if scanned == 0 {
		return
	}

	// ----- 2. Group by size -----
	bySize := make(map[int64][]*FileInfo)
	for i := range allFiles {
		bySize[allFiles[i].Size] = append(bySize[allFiles[i].Size], &allFiles[i])
	}

	// ----- 3. Parallel hashing -----
	var filesToHash []*FileInfo
	for _, files := range bySize {
		if len(files) > 1 {
			filesToHash = append(filesToHash, files...)
		}
	}
	totalToHash := len(filesToHash)
	if totalToHash > 0 {
		fmt.Printf("Parallel hashing %d files (workers: %d)...\n", totalToHash, runtime.NumCPU())
		startHash := time.Now()

		// Start progress tracker
		progress := NewProgressTracker(totalToHash, "files")
		progress.Start()

		jobs := make(chan *FileInfo, totalToHash)
		var wg sync.WaitGroup
		for w := 0; w < runtime.NumCPU(); w++ {
			wg.Add(1)
			go func() {
				defer wg.Done()
				for f := range jobs {
					hash, err := computeSHA256(f.Path)
					if err != nil {
						fmt.Fprintf(os.Stderr, "Hash error %s: %v\n", f.Path, err)
						continue
					}
					f.Hash = hash
					progress.Increment()
				}
			}()
		}
		for _, f := range filesToHash {
			jobs <- f
		}
		close(jobs)
		wg.Wait()
		progress.Stop()
		fmt.Printf("Hashing completed in %v\n", time.Since(startHash))
	}

	// ----- 4. Build duplicate groups -----
	var csvRows [][]string
	bashLines := []string{"#!/bin/bash", "# Generated by find_dups", "set -e", ""}
	duplicatesCount := 0

	for size, files := range bySize {
		if len(files) < 2 {
			continue
		}
		hashGroups := make(map[string][]*FileInfo)
		for _, f := range files {
			if f.Hash != "" {
				hashGroups[f.Hash] = append(hashGroups[f.Hash], f)
			}
		}
		for hash, group := range hashGroups {
			if len(group) < 2 {
				continue
			}
			sort.Slice(group, func(i, j int) bool { return group[i].ID < group[j].ID })
			for _, f := range group {
				csvRows = append(csvRows, []string{
					fmt.Sprint(f.ID),
					f.Path,
					fmt.Sprint(size),
					hash,
					f.Birth.Format(time.RFC3339),
					f.ModTime.Format(time.RFC3339),
				})
			}
			for i, f := range group {
				if i == 0 {
					continue
				}
				duplicatesCount++
				bashLines = append(bashLines, fmt.Sprintf("rm -- %q", f.Path))
			}
		}
	}
	bashLines = append(bashLines, "", `echo "Deletion complete."`)

	// Write CSV
	csvFile, _ := os.Create("duplicates_go.csv")
	defer csvFile.Close()
	csvWriter := csv.NewWriter(csvFile)
	defer csvWriter.Flush()
	csvWriter.Write([]string{"FileID", "Path", "Size", "Hash", "CreationTime", "ModificationTime"})
	csvWriter.WriteAll(csvRows)

	// Write bash script
	shFile, _ := os.Create("duprm_go.sh")
	defer shFile.Close()
	for _, line := range bashLines {
		fmt.Fprintln(shFile, line)
	}
	shFile.Chmod(0755)

	// ----- 5. Sorted CSV (all files) -----
	sorted := make([]FileInfo, len(allFiles))
	copy(sorted, allFiles)
	sort.Slice(sorted, func(i, j int) bool { return sorted[i].Size > sorted[j].Size })
	sortFile, _ := os.Create("sort_dup_go.csv")
	defer sortFile.Close()
	sortWriter := csv.NewWriter(sortFile)
	defer sortWriter.Flush()
	sortWriter.Write([]string{"FileID", "Path", "Size", "Hash", "CreationTime", "ModificationTime"})
	for _, f := range sorted {
		sortWriter.Write([]string{
			fmt.Sprint(f.ID),
			f.Path,
			fmt.Sprint(f.Size),
			f.Hash,
			f.Birth.Format(time.RFC3339),
			f.ModTime.Format(time.RFC3339),
		})
	}

	// ----- 6. Analytics -----
	elapsed := time.Since(startTotal)
	generateAnalytics(allFiles, bySize, elapsed)

	// ----- 7. Statistics -----
	fmt.Printf("\nFiles scanned: %d\n", scanned)
	fmt.Printf("Duplicates found (files to delete): %d\n", duplicatesCount)
	fmt.Printf("Runtime:\n")
	fmt.Printf("  - %.3f seconds\n", elapsed.Seconds())
	fmt.Printf("  - %s\n", formatDuration(elapsed))
	fmt.Println("Reports: duplicates_go.csv, sort_dup_go.csv, analytics_go.json")
	fmt.Println("Delete script: duprm_go.sh")
}

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
