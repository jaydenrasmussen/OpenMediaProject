package api

import (
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"

	"backend/clients"
	"backend/clients/indexers"
	"backend/config"
	"backend/db"
	"backend/models"

	"github.com/gofiber/fiber/v2"
	"github.com/rs/zerolog/log"
)

// DownloadHandler handles the automated download pipeline.
type DownloadHandler struct {
	config    *config.Config
	registry  *indexers.Registry
	fileMover *clients.FileMover
}

// NewDownloadHandler creates a new DownloadHandler.
func NewDownloadHandler(cfg *config.Config, fileMover *clients.FileMover, registry *indexers.Registry) *DownloadHandler {
	return &DownloadHandler{
		config:    cfg,
		registry:  registry,
		fileMover: fileMover,
	}
}

// DownloadMovie handles POST /api/v1/movies/:id/download.
// It searches all enabled indexers for the movie and enqueues the best result.
func (h *DownloadHandler) DownloadMovie(c *fiber.Ctx) error {
	id := c.Params("id")

	movie, err := db.NewMovieRepository().GetByID(id)
	if err != nil {
		return c.Status(fiber.StatusNotFound).JSON(fiber.Map{
			"error":   true,
			"message": "Movie not found",
		})
	}

	// Parse desired quality: query param > item-level override > global config.
	qualityStr := c.Query("quality", "")
	if qualityStr == "" {
		qualityStr = movie.PreferredQuality
	}
	if qualityStr == "" {
		qualityStr = h.config.Downloads.PreferredQuality
	}
	quality := indexers.ParseQuality(qualityStr)
	upgradeUntil := c.Query("upgrade_until", "")

	// Extract year from "YYYY-MM-DD" release date.
	year := 0
	if len(movie.ReleaseDate) >= 4 {
		fmt.Sscanf(movie.ReleaseDate[:4], "%d", &year)
	}

	req := &indexers.SearchRequest{
		Query:     movie.Title,
		MediaType: indexers.MediaTypeMovie,
		Quality:   quality,
		Year:      year,
	}

	results, errs := h.registry.Search(req)
	for _, e := range errs {
		log.Warn().Err(e).Str("title", movie.Title).Msg("Indexer error during movie search")
	}
	results = applyReleaseProfiles(results)

	if len(results) == 0 {
		return c.Status(fiber.StatusNotFound).JSON(fiber.Map{
			"error":   true,
			"message": "No torrents found for this movie",
		})
	}

	best := results[0]

	// Duplicate check: don't re-download if library already has equal or better quality
	if h.fileMover.IsMovieDuplicate(movie, string(best.Quality)) {
		return c.Status(fiber.StatusConflict).JSON(fiber.Map{
			"error":   true,
			"message": "A file of equal or better quality already exists in the library",
		})
	}

	torrentClient, err := buildTorrentClient(h.config)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": fmt.Sprintf("Failed to connect to torrent client: %s", err.Error()),
		})
	}

	if err := grabTorrent(torrentClient, best); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": fmt.Sprintf("Failed to add torrent: %s", err.Error()),
		})
	}

	hash := best.InfoHash

	if hash != "" {
		record := &models.PendingDownload{
			Hash:         hash,
			MediaType:    "movie",
			MediaID:      movie.ID,
			Title:        movie.Title,
			Quality:      string(best.Quality),
			Source:       best.Source,
			Status:       models.DownloadStatusDownloading,
			UpgradeUntil: upgradeUntil,
			AddedAt:      time.Now(),
			UpdatedAt:    time.Now(),
		}
		if err := db.NewDownloadRepository().Save(record); err != nil {
			log.Warn().Err(err).Str("hash", hash).Msg("Failed to save pending download record")
		}
	} else {
		log.Warn().Str("title", movie.Title).Msg("Torrent has no InfoHash; download will not be tracked")
	}

	log.Info().
		Str("title", movie.Title).
		Str("hash", hash).
		Str("quality", string(best.Quality)).
		Str("source", best.Source).
		Msg("Movie download started")

	return c.Status(fiber.StatusAccepted).JSON(fiber.Map{
		"success": true,
		"message": "Download started",
		"title":   movie.Title,
		"quality": string(best.Quality),
		"source":  best.Source,
		"hash":    hash,
	})
}

// DownloadTVShow handles POST /api/v1/tvshows/:id/download.
// Optional query params: season (int), episode (int), quality (string).
func (h *DownloadHandler) DownloadTVShow(c *fiber.Ctx) error {
	id := c.Params("id")

	show, err := db.NewTVShowRepository().GetByID(id)
	if err != nil {
		return c.Status(fiber.StatusNotFound).JSON(fiber.Map{
			"error":   true,
			"message": "TV show not found",
		})
	}

	season := c.QueryInt("season", 0)
	episode := c.QueryInt("episode", 0)

	// Parse desired quality: query param > item-level override > global config.
	qualityStr := c.Query("quality", "")
	if qualityStr == "" {
		qualityStr = show.PreferredQuality
	}
	if qualityStr == "" {
		qualityStr = h.config.Downloads.PreferredQuality
	}
	quality := indexers.ParseQuality(qualityStr)
	upgradeUntil := c.Query("upgrade_until", "")

	// Build a more specific query when season/episode are provided.
	query := show.Name
	if season > 0 && episode > 0 {
		query = fmt.Sprintf("%s S%02dE%02d", show.Name, season, episode)
	} else if season > 0 {
		query = fmt.Sprintf("%s Season %d", show.Name, season)
	}

	req := &indexers.SearchRequest{
		Query:     query,
		MediaType: indexers.MediaTypeTV,
		Quality:   quality,
		Season:    season,
		Episode:   episode,
		TvdbID:    show.ID,
	}

	results, errs := h.registry.Search(req)
	for _, e := range errs {
		log.Warn().Err(e).Str("title", show.Name).Msg("Indexer error during TV show search")
	}
	results = applyReleaseProfiles(results)

	if len(results) == 0 {
		return c.Status(fiber.StatusNotFound).JSON(fiber.Map{
			"error":   true,
			"message": "No torrents found for this TV show",
		})
	}

	best := results[0]

	if season > 0 && episode > 0 {
		ep, epErr := db.NewEpisodeRepository().GetByKey(show.ID, season, episode)
		if epErr == nil && h.fileMover.IsTVDuplicate(show, ep, string(best.Quality)) {
			return c.Status(fiber.StatusConflict).JSON(fiber.Map{
				"error":   true,
				"message": "A file of equal or better quality already exists in the library",
			})
		}
	}

	torrentClient, err := buildTorrentClient(h.config)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": fmt.Sprintf("Failed to connect to torrent client: %s", err.Error()),
		})
	}

	if err := grabTorrent(torrentClient, best); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": fmt.Sprintf("Failed to add torrent: %s", err.Error()),
		})
	}

	hash := best.InfoHash

	if hash != "" {
		record := &models.PendingDownload{
			Hash:         hash,
			MediaType:    "tv",
			MediaID:      show.ID,
			Title:        show.Name,
			Quality:      string(best.Quality),
			Source:       best.Source,
			Status:       models.DownloadStatusDownloading,
			UpgradeUntil: upgradeUntil,
			AddedAt:      time.Now(),
			UpdatedAt:    time.Now(),
		}
		if err := db.NewDownloadRepository().Save(record); err != nil {
			log.Warn().Err(err).Str("hash", hash).Msg("Failed to save pending download record")
		}
	} else {
		log.Warn().Str("title", show.Name).Msg("Torrent has no InfoHash; download will not be tracked")
	}

	log.Info().
		Str("title", show.Name).
		Str("hash", hash).
		Str("quality", string(best.Quality)).
		Str("source", best.Source).
		Int("season", season).
		Int("episode", episode).
		Msg("TV show download started")

	return c.Status(fiber.StatusAccepted).JSON(fiber.Map{
		"success": true,
		"message": "Download started",
		"title":   show.Name,
		"quality": string(best.Quality),
		"source":  best.Source,
		"hash":    hash,
	})
}

// GetDownloads handles GET /api/v1/downloads.
func (h *DownloadHandler) GetDownloads(c *fiber.Ctx) error {
	downloads, err := db.NewDownloadRepository().GetAll()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": fmt.Sprintf("Failed to retrieve downloads: %s", err.Error()),
		})
	}

	return c.JSON(fiber.Map{
		"downloads": downloads,
		"count":     len(downloads),
	})
}

// GetDownload handles GET /api/v1/downloads/:hash.
func (h *DownloadHandler) GetDownload(c *fiber.Ctx) error {
	hash := c.Params("hash")

	download, err := db.NewDownloadRepository().GetByHash(hash)
	if err != nil {
		return c.Status(fiber.StatusNotFound).JSON(fiber.Map{
			"error":   true,
			"message": "Download not found",
		})
	}

	return c.JSON(download)
}

// grabTorrent sends the best search result to the torrent client, preferring
// a magnet link, then falling back to downloading the .torrent file.
func grabTorrent(client clients.TorrentClient, result *indexers.SearchResult) error {
	if result.MagnetURL != "" {
		return client.AddTorrentURL(result.MagnetURL, true)
	}

	if result.DownloadURL != "" {
		httpClient := &http.Client{Timeout: 30 * time.Second}
		resp, err := httpClient.Get(result.DownloadURL)
		if err != nil {
			return fmt.Errorf("failed to fetch torrent file: %w", err)
		}
		defer resp.Body.Close()

		data, err := io.ReadAll(resp.Body)
		if err != nil {
			return fmt.Errorf("failed to read torrent file body: %w", err)
		}

		filename := result.Title + ".torrent"
		return client.AddTorrentFile(data, filename, true)
	}

	return fmt.Errorf("no download URL or magnet available")
}

// buildTorrentClient constructs a TorrentClient from the application config.
func buildTorrentClient(cfg *config.Config) (clients.TorrentClient, error) {
	clientConfig := clients.TorrentClientConfig{
		ClientType: clients.TorrentClientType(cfg.TorrentClient.ClientType),
		BaseURL:    cfg.TorrentClient.BaseURL,
		Username:   cfg.TorrentClient.Username,
		Password:   cfg.TorrentClient.Password,
	}
	return clients.NewTorrentClient(clientConfig)
}

// applyReleaseProfiles filters and re-orders results according to all enabled
// release profiles stored in the database. It queries the database on every
// call but returns early when no profiles are enabled.
//
// Profile semantics (OR across profiles, AND within each profile):
//   - A result passes if it satisfies AT LEAST ONE enabled profile.
//   - Within a single profile: title must contain ALL Require terms AND none of the Reject terms.
//   - Prefer terms from all profiles are aggregated; matching results are sorted to the front.
func applyReleaseProfiles(results []*indexers.SearchResult) []*indexers.SearchResult {
	profiles, err := db.NewReleaseProfileRepository().GetEnabled()
	if err != nil || len(profiles) == 0 {
		return results
	}

	// Collect all prefer terms globally — used for ordering only, not filtering.
	var preferTerms []string
	for _, p := range profiles {
		preferTerms = append(preferTerms, p.Prefer...)
	}

	filtered := make([]*indexers.SearchResult, 0, len(results))
	for _, r := range results {
		if satisfiesAnyProfile(strings.ToLower(r.Title), profiles) {
			filtered = append(filtered, r)
		}
	}

	if len(preferTerms) == 0 {
		return filtered
	}

	// Stable partition: results matching any prefer term first.
	preferred := make([]*indexers.SearchResult, 0, len(filtered))
	rest := make([]*indexers.SearchResult, 0, len(filtered))
	for _, r := range filtered {
		title := strings.ToLower(r.Title)
		matched := false
		for _, term := range preferTerms {
			if strings.Contains(title, strings.ToLower(term)) {
				matched = true
				break
			}
		}
		if matched {
			preferred = append(preferred, r)
		} else {
			rest = append(rest, r)
		}
	}
	return append(preferred, rest...)
}

// satisfiesAnyProfile reports whether the lower-cased release title passes
// at least one enabled profile's Require/Reject rules.
func satisfiesAnyProfile(titleLower string, profiles []*models.ReleaseProfile) bool {
	for _, p := range profiles {
		if profileMatches(titleLower, p) {
			return true
		}
	}
	return false
}

// profileMatches reports whether a title passes a single profile's rules:
// it must not contain any Reject term, and must contain all Require terms.
func profileMatches(titleLower string, p *models.ReleaseProfile) bool {
	for _, term := range p.Reject {
		if strings.Contains(titleLower, strings.ToLower(term)) {
			return false
		}
	}
	for _, term := range p.Require {
		if !strings.Contains(titleLower, strings.ToLower(term)) {
			return false
		}
	}
	return true
}
