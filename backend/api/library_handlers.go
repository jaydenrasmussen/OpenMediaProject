package api

import (
	"os"
	"path/filepath"
	"strings"
	"time"

	"backend/clients"
	"backend/config"
	"backend/db"

	"github.com/gofiber/fiber/v2"
)

// videoExts is the set of extensions recognised as video files for library scanning.
var videoExts = map[string]bool{
	".mkv": true, ".mp4": true, ".avi": true, ".mov": true,
	".wmv": true, ".m4v": true, ".ts": true, ".m2ts": true, ".webm": true,
}

// LibraryHandler handles library scanning and reconciliation.
type LibraryHandler struct {
	config    *config.Config
	fileMover *clients.FileMover
}

// NewLibraryHandler creates a new LibraryHandler.
func NewLibraryHandler(cfg *config.Config, fileMover *clients.FileMover) *LibraryHandler {
	return &LibraryHandler{config: cfg, fileMover: fileMover}
}

// libraryFile is a single video file entry in a scan result.
type libraryFile struct {
	Path    string `json:"path"`
	Title   string `json:"title,omitempty"`
	Size    int64  `json:"size"`
	ModTime string `json:"mod_time,omitempty"`
}

// scanResult holds the matched and unmatched file lists for one media type.
type scanResult struct {
	Matched   []libraryFile `json:"matched"`
	Unmatched []libraryFile `json:"unmatched"`
}

// ScanLibrary godoc
//
//	GET /api/v1/library/scan
//
// Walks the configured movie and TV show directories, records every video file
// found, and attempts to match each one against a DB record by checking whether
// any known title appears in the file path.
func (h *LibraryHandler) ScanLibrary(c *fiber.Ctx) error {
	movies, err := h.scanMovieDir()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to scan movie directory: " + err.Error(),
		})
	}

	tvShows, err := h.scanTVDir()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to scan TV show directory: " + err.Error(),
		})
	}

	return c.JSON(fiber.Map{
		"movies":     movies,
		"tvshows":    tvShows,
		"scanned_at": time.Now().UTC().Format(time.RFC3339),
	})
}

// scanMovieDir walks cfg.FileMover.MovieDir and matches files against the DB.
func (h *LibraryHandler) scanMovieDir() (*scanResult, error) {
	result := &scanResult{
		Matched:   []libraryFile{},
		Unmatched: []libraryFile{},
	}

	dir := h.config.FileMover.MovieDir
	if dir == "" {
		return result, nil
	}
	if _, err := os.Stat(dir); os.IsNotExist(err) {
		return result, nil
	}

	movieRepo := db.NewMovieRepository()
	allMovies, err := movieRepo.GetAll()
	if err != nil {
		// Non-fatal — scan without DB matches.
		allMovies = nil
	}

	err = filepath.Walk(dir, func(path string, info os.FileInfo, werr error) error {
		if werr != nil || info.IsDir() {
			return nil
		}
		ext := strings.ToLower(filepath.Ext(path))
		if !videoExts[ext] {
			return nil
		}
		fi := libraryFile{
			Path:    path,
			Size:    info.Size(),
			ModTime: info.ModTime().UTC().Format(time.RFC3339),
		}
		pathLower := strings.ToLower(path)
		matched := false
		for _, m := range allMovies {
			if m.Title != "" && strings.Contains(pathLower, strings.ToLower(m.Title)) {
				fi.Title = m.Title
				result.Matched = append(result.Matched, fi)
				matched = true
				break
			}
		}
		if !matched {
			result.Unmatched = append(result.Unmatched, fi)
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	return result, nil
}

// scanTVDir walks cfg.FileMover.TVShowDir and matches files against the DB.
func (h *LibraryHandler) scanTVDir() (*scanResult, error) {
	result := &scanResult{
		Matched:   []libraryFile{},
		Unmatched: []libraryFile{},
	}

	dir := h.config.FileMover.TVShowDir
	if dir == "" {
		return result, nil
	}
	if _, err := os.Stat(dir); os.IsNotExist(err) {
		return result, nil
	}

	showRepo := db.NewTVShowRepository()
	allShows, err := showRepo.GetAll()
	if err != nil {
		allShows = nil
	}

	err = filepath.Walk(dir, func(path string, info os.FileInfo, werr error) error {
		if werr != nil || info.IsDir() {
			return nil
		}
		ext := strings.ToLower(filepath.Ext(path))
		if !videoExts[ext] {
			return nil
		}
		fi := libraryFile{
			Path:    path,
			Size:    info.Size(),
			ModTime: info.ModTime().UTC().Format(time.RFC3339),
		}
		pathLower := strings.ToLower(path)
		matched := false
		for _, s := range allShows {
			if s.Name != "" && strings.Contains(pathLower, strings.ToLower(s.Name)) {
				fi.Title = s.Name
				result.Matched = append(result.Matched, fi)
				matched = true
				break
			}
		}
		if !matched {
			result.Unmatched = append(result.Unmatched, fi)
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	return result, nil
}
