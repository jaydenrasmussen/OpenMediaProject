package api

import (
	"time"

	"backend/clients/indexers"
	"backend/config"
	"backend/db"
	"backend/models"

	"github.com/gofiber/fiber/v2"
	"github.com/google/uuid"
)

// WantedHandler handles wanted queue API operations.
type WantedHandler struct {
	config   *config.Config
	registry *indexers.Registry
}

// NewWantedHandler creates a new WantedHandler.
func NewWantedHandler(cfg *config.Config) *WantedHandler {
	return &WantedHandler{
		config:   cfg,
		registry: indexers.NewDefaultRegistry(),
	}
}

// ListWanted handles GET /api/v1/wanted.
// Optional ?status= query param to filter by status.
func (h *WantedHandler) ListWanted(c *fiber.Ctx) error {
	repo := db.NewWantedRepository()

	statusFilter := c.Query("status", "")

	var (
		items []*models.WantedItem
		err   error
	)

	if statusFilter != "" {
		items, err = repo.GetByStatus(statusFilter)
	} else {
		items, err = repo.GetAll()
	}

	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to retrieve wanted items",
		})
	}

	// Return an empty array rather than null when there are no items.
	if items == nil {
		items = []*models.WantedItem{}
	}

	return c.JSON(fiber.Map{
		"items": items,
		"count": len(items),
	})
}

// GetWanted handles GET /api/v1/wanted/:id.
func (h *WantedHandler) GetWanted(c *fiber.Ctx) error {
	id := c.Params("id")

	item, err := db.NewWantedRepository().GetByID(id)
	if err != nil {
		if err == db.ErrWantedNotFound {
			return c.Status(fiber.StatusNotFound).JSON(fiber.Map{
				"error":   true,
				"message": "Wanted item not found",
			})
		}
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to retrieve wanted item",
		})
	}

	return c.JSON(item)
}

// DeleteWanted handles DELETE /api/v1/wanted/:id.
func (h *WantedHandler) DeleteWanted(c *fiber.Ctx) error {
	id := c.Params("id")

	repo := db.NewWantedRepository()

	if _, err := repo.GetByID(id); err != nil {
		return c.Status(fiber.StatusNotFound).JSON(fiber.Map{
			"error":   true,
			"message": "Wanted item not found",
		})
	}

	if err := repo.Delete(id); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to delete wanted item",
		})
	}

	return c.SendStatus(fiber.StatusNoContent)
}

// requestMovieBody is the optional JSON body for POST /api/v1/movies/:id/request.
type requestMovieBody struct {
	MinQuality   string `json:"min_quality"`
	UpgradeUntil string `json:"upgrade_until"`
}

// RequestMovie handles POST /api/v1/movies/:id/request.
func (h *WantedHandler) RequestMovie(c *fiber.Ctx) error {
	id := c.Params("id")

	movie, err := db.NewMovieRepository().GetByID(id)
	if err != nil {
		return c.Status(fiber.StatusNotFound).JSON(fiber.Map{
			"error":   true,
			"message": "Movie not found",
		})
	}

	// Check whether this movie is already being tracked.
	existing, err := db.NewWantedRepository().GetByMedia("movie", id)
	if err == nil {
		for _, e := range existing {
			if e.Status == models.WantedStatusWanted || e.Status == models.WantedStatusDownloading {
				return c.Status(fiber.StatusConflict).JSON(fiber.Map{
					"error":   true,
					"message": "Already in wanted queue",
				})
			}
		}
	}

	// Parse optional body.
	var body requestMovieBody
	// Ignore parse errors — all fields are optional.
	_ = c.BodyParser(&body)

	minQuality := body.MinQuality
	if minQuality == "" {
		minQuality = h.config.Downloads.PreferredQuality
	}

	now := time.Now()
	item := &models.WantedItem{
		ID:           uuid.NewString(),
		MediaType:    "movie",
		MediaID:      movie.ID,
		Title:        movie.Title,
		MinQuality:   minQuality,
		UpgradeUntil: body.UpgradeUntil,
		Status:       models.WantedStatusWanted,
		AddedAt:      now,
		UpdatedAt:    now,
	}

	if err := db.NewWantedRepository().Save(item); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to save wanted item",
		})
	}

	return c.Status(fiber.StatusCreated).JSON(fiber.Map{
		"success": true,
		"message": "Added to wanted queue",
		"id":      item.ID,
	})
}

// requestTVBody is the optional JSON body for POST /api/v1/tvshows/:id/request.
type requestTVBody struct {
	Season       int    `json:"season"`
	Episode      int    `json:"episode"`
	MinQuality   string `json:"min_quality"`
	UpgradeUntil string `json:"upgrade_until"`
}

// RequestTVShow handles POST /api/v1/tvshows/:id/request.
func (h *WantedHandler) RequestTVShow(c *fiber.Ctx) error {
	id := c.Params("id")

	show, err := db.NewTVShowRepository().GetByID(id)
	if err != nil {
		return c.Status(fiber.StatusNotFound).JSON(fiber.Map{
			"error":   true,
			"message": "TV show not found",
		})
	}

	// Parse optional body.
	var body requestTVBody
	_ = c.BodyParser(&body)

	// Check for duplicates — same show + season + episode already wanted/downloading.
	existing, err := db.NewWantedRepository().GetByMedia("tv", id)
	if err == nil {
		for _, e := range existing {
			if e.Status != models.WantedStatusWanted && e.Status != models.WantedStatusDownloading {
				continue
			}
			if e.Season == body.Season && e.Episode == body.Episode {
				return c.Status(fiber.StatusConflict).JSON(fiber.Map{
					"error":   true,
					"message": "Already in wanted queue",
				})
			}
		}
	}

	minQuality := body.MinQuality
	if minQuality == "" {
		minQuality = h.config.Downloads.PreferredQuality
	}

	now := time.Now()
	item := &models.WantedItem{
		ID:           uuid.NewString(),
		MediaType:    "tv",
		MediaID:      show.ID,
		Title:        show.Name,
		Season:       body.Season,
		Episode:      body.Episode,
		MinQuality:   minQuality,
		UpgradeUntil: body.UpgradeUntil,
		Status:       models.WantedStatusWanted,
		AddedAt:      now,
		UpdatedAt:    now,
	}

	if err := db.NewWantedRepository().Save(item); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to save wanted item",
		})
	}

	return c.Status(fiber.StatusCreated).JSON(fiber.Map{
		"success": true,
		"message": "Added to wanted queue",
		"id":      item.ID,
	})
}

// ApproveWanted handles POST /api/v1/wanted/:id/approve (admin only).
// Transitions a pending item to "wanted" so the poller will process it.
func (h *WantedHandler) ApproveWanted(c *fiber.Ctx) error {
	id := c.Params("id")
	repo := db.NewWantedRepository()

	item, err := repo.GetByID(id)
	if err != nil {
		if err == db.ErrWantedNotFound {
			return c.Status(fiber.StatusNotFound).JSON(fiber.Map{"error": true, "message": "Wanted item not found"})
		}
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{"error": true, "message": err.Error()})
	}

	if item.Status != models.WantedStatusPending {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Only pending items can be approved",
		})
	}

	if err := repo.UpdateStatus(id, models.WantedStatusWanted); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{"error": true, "message": err.Error()})
	}

	return c.JSON(fiber.Map{"success": true, "message": "Wanted item approved"})
}

// RejectWanted handles POST /api/v1/wanted/:id/reject (admin only).
// Transitions a pending item to "rejected".
func (h *WantedHandler) RejectWanted(c *fiber.Ctx) error {
	id := c.Params("id")
	repo := db.NewWantedRepository()

	item, err := repo.GetByID(id)
	if err != nil {
		if err == db.ErrWantedNotFound {
			return c.Status(fiber.StatusNotFound).JSON(fiber.Map{"error": true, "message": "Wanted item not found"})
		}
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{"error": true, "message": err.Error()})
	}

	if item.Status != models.WantedStatusPending {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Only pending items can be rejected",
		})
	}

	if err := repo.UpdateStatus(id, models.WantedStatusRejected); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{"error": true, "message": err.Error()})
	}

	return c.JSON(fiber.Map{"success": true, "message": "Wanted item rejected"})
}
