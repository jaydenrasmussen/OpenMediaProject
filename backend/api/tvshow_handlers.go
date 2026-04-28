package api

import (
	"log"
	"strconv"
	"time"

	"backend/clients"
	"backend/config"
	"backend/db"
	"backend/models"

	"github.com/gofiber/fiber/v2"
)

// TVShowResponse represents a response containing TV show data
type TVShowResponse struct {
	Shows []*models.TVShow `json:"shows"`
	Count int              `json:"count"`
}

// GetTVShows returns all TV shows in the database
func GetTVShows(c *fiber.Ctx) error {
	repo := db.NewTVShowRepository()
	shows, err := repo.GetAll()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error retrieving TV shows: " + err.Error(),
		})
	}

	count, _ := repo.Count()

	return c.JSON(TVShowResponse{
		Shows: shows,
		Count: count,
	})
}

// GetTVShow returns a specific TV show by ID
func GetTVShow(c *fiber.Ctx) error {
	id := c.Params("id")
	repo := db.NewTVShowRepository()

	show, err := repo.GetByID(id)
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

	return c.JSON(show)
}

// FetchPopularTVShows fetches popular TV shows from TVDB API and stores them in the database
func FetchPopularTVShows(c *fiber.Ctx) error {
	// Get configuration
	cfg, err := config.Load()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to load configuration: " + err.Error(),
		})
	}

	// Check if TVDB API key is configured
	if cfg.TVDB.APIKey == "" {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "TVDB API key is not configured",
		})
	}

	// Create TVDB client
	client := clients.NewTVDBClient(cfg.TVDB.APIKey)

	// Fetch popular shows
	rawShows, err := client.GetPopularShows()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to fetch popular TV shows: " + err.Error(),
		})
	}

	// Convert to our model
	shows := make([]*models.TVShow, 0, len(rawShows))
	for _, rawShow := range rawShows {
		show := models.NewTVShowFromTVDBData(rawShow)
		if show.ID != "" { // Only add shows with valid IDs
			shows = append(shows, show)
		}
	}

	// Save to database
	repo := db.NewTVShowRepository()
	if err := repo.UpsertBatch(shows); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to save TV shows: " + err.Error(),
		})
	}

	return c.JSON(fiber.Map{
		"success": true,
		"message": "Successfully fetched and stored popular TV shows",
		"count":   len(shows),
	})
}

// SearchTVShows searches for TV shows by name using TVDB API
func SearchTVShows(c *fiber.Ctx) error {
	query := c.Query("q")
	if query == "" {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Search query is required",
		})
	}

	// Get configuration
	cfg, err := config.Load()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to load configuration: " + err.Error(),
		})
	}

	// Check if TVDB API key is configured
	if cfg.TVDB.APIKey == "" {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "TVDB API key is not configured",
		})
	}

	// Create TVDB client
	client := clients.NewTVDBClient(cfg.TVDB.APIKey)

	// Search for shows
	rawShows, err := client.SearchShows(query)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to search TV shows: " + err.Error(),
		})
	}

	// Convert to our model
	shows := make([]*models.TVShow, 0, len(rawShows))
	for _, rawShow := range rawShows {
		show := models.NewTVShowFromTVDBData(rawShow)
		if show.ID != "" { // Only add shows with valid IDs
			shows = append(shows, show)
		}
	}

	// Save to database
	repo := db.NewTVShowRepository()
	if err := repo.UpsertBatch(shows); err != nil {
		log.Printf("Failed to save searched TV shows: %v", err)
		// We continue anyway to return search results
	}

	return c.JSON(TVShowResponse{
		Shows: shows,
		Count: len(shows),
	})
}

// GetRecentTVShows returns TV shows that were recently updated
func GetRecentTVShows(c *fiber.Ctx) error {
	// Parse days parameter (default to 7 days)
	daysStr := c.Query("days", "7")
	days, err := strconv.Atoi(daysStr)
	if err != nil || days <= 0 {
		days = 7
	}

	// Calculate cutoff time
	cutoff := time.Now().Add(-time.Duration(days) * 24 * time.Hour)

	// Get recent shows
	repo := db.NewTVShowRepository()
	shows, err := repo.GetRecentlyUpdated(cutoff)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error retrieving recent TV shows: " + err.Error(),
		})
	}

	return c.JSON(TVShowResponse{
		Shows: shows,
		Count: len(shows),
	})
}

// DeleteTVShow deletes a TV show by ID
func DeleteTVShow(c *fiber.Ctx) error {
	id := c.Params("id")
	repo := db.NewTVShowRepository()

	// Check if TV show exists
	_, err := repo.GetByID(id)
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

	// Delete the TV show
	if err := repo.Delete(id); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error deleting TV show: " + err.Error(),
		})
	}

	return c.SendStatus(fiber.StatusNoContent)
}

// SetTVShowQuality handles PATCH /api/v1/tvshows/:id/quality.
// It sets (or clears) the per-item preferred quality override.
func SetTVShowQuality(c *fiber.Ctx) error {
	id := c.Params("id")

	var body struct {
		Quality string `json:"quality"`
	}
	if err := c.BodyParser(&body); err != nil {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Invalid request body: " + err.Error(),
		})
	}

	repo := db.NewTVShowRepository()
	show, err := repo.GetByID(id)
	if err != nil {
		if err == db.ErrTVShowNotFound {
			return c.Status(fiber.StatusNotFound).JSON(fiber.Map{
				"error":   true,
				"message": "TV show not found",
			})
		}
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to retrieve TV show: " + err.Error(),
		})
	}

	show.PreferredQuality = body.Quality

	if err := repo.Update(show); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to update TV show: " + err.Error(),
		})
	}

	return c.JSON(fiber.Map{
		"success": true,
		"quality": show.PreferredQuality,
	})
}
