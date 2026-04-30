package api

import (
	"strconv"
	"time"

	"backend/clients"
	"backend/config"
	"backend/db"
	"backend/models"

	"github.com/gofiber/fiber/v2"
	"github.com/google/uuid"
	"github.com/rs/zerolog/log"
)

// EpisodeHandler handles episode-related API requests.
type EpisodeHandler struct {
	config *config.Config
}

// NewEpisodeHandler creates a new EpisodeHandler.
func NewEpisodeHandler(cfg *config.Config) *EpisodeHandler {
	return &EpisodeHandler{config: cfg}
}

// episodesResponse is the standard envelope returned by episode list endpoints.
type episodesResponse struct {
	ShowID   string            `json:"show_id"`
	Season   *int              `json:"season,omitempty"`
	Episodes []*models.Episode `json:"episodes"`
	Count    int               `json:"count"`
}

// fetchAndSaveFromTVDB fetches all episodes for a show from TVDB, saves them,
// and returns the full list. It does NOT overwrite existing status/quality/DownloadedAt.
func (h *EpisodeHandler) fetchAndSaveFromTVDB(showID string) ([]*models.Episode, error) {
	client := clients.NewTVDBClient(h.config.TVDB.APIKey)
	rawEpisodes, err := client.GetEpisodes(showID)
	if err != nil {
		return nil, err
	}

	epRepo := db.NewEpisodeRepository()
	now := time.Now()
	today := now.Truncate(24 * time.Hour)
	toSave := make([]*models.Episode, 0, len(rawEpisodes))

	for _, raw := range rawEpisodes {
		ep := models.NewEpisodeFromTVDBData(showID, raw)
		if ep == nil {
			continue
		}

		existing, getErr := epRepo.GetByKey(showID, ep.Season, ep.Number)
		if getErr == nil && existing != nil {
			// Preserve tracked state — only update metadata
			existing.Title = ep.Title
			existing.AirDate = ep.AirDate
			existing.Overview = ep.Overview
			existing.FetchedAt = now
			toSave = append(toSave, existing)
		} else {
			// New episode — determine status from air date
			ep.FetchedAt = now
			ep.Status = episodeStatusFromAirDate(ep.AirDate, today)
			toSave = append(toSave, ep)
		}
	}

	if err := epRepo.SaveBatch(toSave); err != nil {
		return nil, err
	}
	return toSave, nil
}

// episodeStatusFromAirDate returns EpisodeStatusMissing when the episode has
// already aired (airDate <= today), EpisodeStatusWanted when it airs in the
// future, and EpisodeStatusMissing when the air date is empty or unparseable
// (unknown air date ≠ "coming soon").
func episodeStatusFromAirDate(airDate string, today time.Time) string {
	if airDate == "" {
		return models.EpisodeStatusMissing
	}
	t, err := time.Parse("2006-01-02", airDate)
	if err != nil {
		return models.EpisodeStatusMissing
	}
	if t.After(today) {
		return models.EpisodeStatusWanted
	}
	return models.EpisodeStatusMissing
}

// GetEpisodes handles GET /api/v1/tvshows/:id/episodes
func (h *EpisodeHandler) GetEpisodes(c *fiber.Ctx) error {
	showID := c.Params("id")

	// 1. Verify show exists
	showRepo := db.NewTVShowRepository()
	if _, err := showRepo.GetByID(showID); err != nil {
		if err == db.ErrTVShowNotFound {
			return c.Status(fiber.StatusNotFound).JSON(fiber.Map{
				"error":   true,
				"message": "TV show not found",
			})
		}
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error retrieving TV show: " + err.Error(),
		})
	}

	// 2. Load episodes from DB
	epRepo := db.NewEpisodeRepository()
	episodes, err := epRepo.GetByShow(showID)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error retrieving episodes: " + err.Error(),
		})
	}

	// 3. On-demand fetch from TVDB when DB is empty and API key is configured
	if len(episodes) == 0 && h.config.TVDB.APIKey != "" {
		fetched, err := h.fetchAndSaveFromTVDB(showID)
		if err != nil {
			return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
				"error":   true,
				"message": "Error fetching episodes from TVDB: " + err.Error(),
			})
		}
		episodes = fetched
	}

	// 4. Optional status filter
	if statusFilter := c.Query("status"); statusFilter != "" {
		filtered := make([]*models.Episode, 0, len(episodes))
		for _, ep := range episodes {
			if ep.Status == statusFilter {
				filtered = append(filtered, ep)
			}
		}
		episodes = filtered
	}

	return c.JSON(episodesResponse{
		ShowID:   showID,
		Episodes: episodes,
		Count:    len(episodes),
	})
}

// GetSeasonEpisodes handles GET /api/v1/tvshows/:id/seasons/:season/episodes
func (h *EpisodeHandler) GetSeasonEpisodes(c *fiber.Ctx) error {
	showID := c.Params("id")
	seasonStr := c.Params("season")
	season, err := strconv.Atoi(seasonStr)
	if err != nil {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Invalid season number",
		})
	}

	// Verify show exists
	showRepo := db.NewTVShowRepository()
	if _, err := showRepo.GetByID(showID); err != nil {
		if err == db.ErrTVShowNotFound {
			return c.Status(fiber.StatusNotFound).JSON(fiber.Map{
				"error":   true,
				"message": "TV show not found",
			})
		}
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error retrieving TV show: " + err.Error(),
		})
	}

	epRepo := db.NewEpisodeRepository()
	episodes, err := epRepo.GetBySeason(showID, season)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error retrieving episodes: " + err.Error(),
		})
	}

	// On-demand TVDB fetch; filter to requested season afterwards
	if len(episodes) == 0 && h.config.TVDB.APIKey != "" {
		fetched, err := h.fetchAndSaveFromTVDB(showID)
		if err != nil {
			return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
				"error":   true,
				"message": "Error fetching episodes from TVDB: " + err.Error(),
			})
		}
		for _, ep := range fetched {
			if ep.Season == season {
				episodes = append(episodes, ep)
			}
		}
	}

	// Optional status filter
	if statusFilter := c.Query("status"); statusFilter != "" {
		filtered := make([]*models.Episode, 0, len(episodes))
		for _, ep := range episodes {
			if ep.Status == statusFilter {
				filtered = append(filtered, ep)
			}
		}
		episodes = filtered
	}

	seasonVal := season
	return c.JSON(episodesResponse{
		ShowID:   showID,
		Season:   &seasonVal,
		Episodes: episodes,
		Count:    len(episodes),
	})
}

// updateEpisodeBody is the request body for UpdateEpisode.
type updateEpisodeBody struct {
	Status  string `json:"status"`
	Quality string `json:"quality"`
}

// UpdateEpisode handles PUT /api/v1/tvshows/:id/episodes/:eid
func (h *EpisodeHandler) UpdateEpisode(c *fiber.Ctx) error {
	showID := c.Params("id")
	eid := c.Params("eid")

	var body updateEpisodeBody
	if err := c.BodyParser(&body); err != nil {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Invalid request body: " + err.Error(),
		})
	}

	// Load all episodes for the show and find the one matching eid
	epRepo := db.NewEpisodeRepository()
	all, err := epRepo.GetByShow(showID)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error retrieving episodes: " + err.Error(),
		})
	}

	var target *models.Episode
	for _, ep := range all {
		if ep.ID == eid {
			target = ep
			break
		}
	}

	if target == nil {
		return c.Status(fiber.StatusNotFound).JSON(fiber.Map{
			"error":   true,
			"message": "Episode not found",
		})
	}

	// Apply non-empty fields
	if body.Status != "" {
		target.Status = body.Status
	}
	if body.Quality != "" {
		target.Quality = body.Quality
	}

	if err := epRepo.Save(target); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error saving episode: " + err.Error(),
		})
	}

	return c.JSON(target)
}

// RefreshEpisodes handles POST /api/v1/tvshows/:id/episodes/refresh
func (h *EpisodeHandler) RefreshEpisodes(c *fiber.Ctx) error {
	showID := c.Params("id")

	// 1. Verify show exists
	showRepo := db.NewTVShowRepository()
	if _, err := showRepo.GetByID(showID); err != nil {
		if err == db.ErrTVShowNotFound {
			return c.Status(fiber.StatusNotFound).JSON(fiber.Map{
				"error":   true,
				"message": "TV show not found",
			})
		}
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error retrieving TV show: " + err.Error(),
		})
	}

	if h.config.TVDB.APIKey == "" {
		return c.Status(fiber.StatusServiceUnavailable).JSON(fiber.Map{
			"error":   true,
			"message": "TVDB API key is not configured",
		})
	}

	// 2. Fetch from TVDB
	client := clients.NewTVDBClient(h.config.TVDB.APIKey)
	rawEpisodes, err := client.GetEpisodes(showID)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error fetching episodes from TVDB: " + err.Error(),
		})
	}

	epRepo := db.NewEpisodeRepository()
	now := time.Now()
	today := now.Truncate(24 * time.Hour)
	toSave := make([]*models.Episode, 0, len(rawEpisodes))

	// 3. Merge: preserve status/quality/DownloadedAt for existing episodes
	for _, raw := range rawEpisodes {
		ep := models.NewEpisodeFromTVDBData(showID, raw)
		if ep == nil {
			continue
		}

		existing, err := epRepo.GetByKey(showID, ep.Season, ep.Number)
		if err == nil && existing != nil {
			// Update metadata only
			existing.Title = ep.Title
			existing.AirDate = ep.AirDate
			existing.Overview = ep.Overview
			existing.FetchedAt = now
			toSave = append(toSave, existing)
		} else {
			// New episode — determine status from air date
			ep.Status = episodeStatusFromAirDate(ep.AirDate, today)
			ep.FetchedAt = now
			toSave = append(toSave, ep)
		}
	}

	// 4. SaveBatch
	if err := epRepo.SaveBatch(toSave); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error saving episodes: " + err.Error(),
		})
	}

	// 5. Return success
	return c.JSON(fiber.Map{
		"success": true,
		"count":   len(toSave),
	})
}

// MonitorTVShow handles POST /api/v1/tvshows/:id/monitor.
// It marks the show as monitored, fetches all episodes from TVDB, and adds
// every episode that has not yet been downloaded to the wanted queue.
func (h *EpisodeHandler) MonitorTVShow(c *fiber.Ctx) error {
	showID := c.Params("id")

	// 1. Verify show exists and mark it monitored.
	showRepo := db.NewTVShowRepository()
	show, err := showRepo.GetByID(showID)
	if err != nil {
		if err == db.ErrTVShowNotFound {
			return c.Status(fiber.StatusNotFound).JSON(fiber.Map{
				"error":   true,
				"message": "TV show not found",
			})
		}
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error retrieving TV show: " + err.Error(),
		})
	}

	// 2. Fetch/refresh episodes from TVDB.
	if h.config.TVDB.APIKey == "" {
		return c.Status(fiber.StatusServiceUnavailable).JSON(fiber.Map{
			"error":   true,
			"message": "TVDB API key is not configured",
		})
	}

	show.Monitored = true
	if err := showRepo.Update(show); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error saving show: " + err.Error(),
		})
	}

	episodes, err := h.fetchAndSaveFromTVDB(showID)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error fetching episodes from TVDB: " + err.Error(),
		})
	}

	// 3. Build a set of (season, episode) pairs already active in the wanted queue.
	wantedRepo := db.NewWantedRepository()
	existing, _ := wantedRepo.GetByMedia("tv", showID)
	type epKey struct{ season, episode int }
	activeSet := make(map[epKey]bool)
	for _, w := range existing {
		if w.Status == models.WantedStatusWanted || w.Status == models.WantedStatusDownloading {
			activeSet[epKey{w.Season, w.Episode}] = true
		}
	}

	// 4. Enqueue every episode that hasn't been downloaded or skipped.
	now := time.Now()
	added := 0
	for _, ep := range episodes {
		if ep.Status == models.EpisodeStatusDownloaded ||
			ep.Status == models.EpisodeStatusSkipped ||
			ep.Status == models.EpisodeStatusDownloading {
			continue
		}
		if activeSet[epKey{ep.Season, ep.Number}] {
			continue
		}
		item := &models.WantedItem{
			ID:         uuid.NewString(),
			MediaType:  "tv",
			MediaID:    showID,
			Title:      show.Name,
			Season:     ep.Season,
			Episode:    ep.Number,
			MinQuality: h.config.Downloads.PreferredQuality,
			Status:     models.WantedStatusWanted,
			AddedAt:    now,
			UpdatedAt:  now,
		}
		if err := wantedRepo.Save(item); err != nil {
			log.Warn().Err(err).Str("showID", showID).Int("season", ep.Season).Int("episode", ep.Number).Msg("MonitorTVShow: failed to save wanted item")
			continue
		}
		added++
	}

	return c.JSON(fiber.Map{
		"success":   true,
		"monitored": true,
		"episodes":  len(episodes),
		"queued":    added,
	})
}
