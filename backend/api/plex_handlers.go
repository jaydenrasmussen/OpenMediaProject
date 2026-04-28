package api

import (
	"backend/db"
	"backend/models"

	"github.com/gofiber/fiber/v2"
)

// PlexHandler holds the dependencies for the Plex handlers.
type PlexHandler struct {
	repo *db.PlexRepository
}

// NewPlexHandler creates a new PlexHandler.
func NewPlexHandler(repo *db.PlexRepository) *PlexHandler {
	return &PlexHandler{repo: repo}
}

// GetPlexConfig retrieves the Plex client configuration.
func (h *PlexHandler) GetPlexConfig(c *fiber.Ctx) error {
	config, err := h.repo.GetPlexConfig()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to get Plex configuration: " + err.Error(),
		})
	}

	return c.JSON(config)
}

// UpdatePlexConfig updates the Plex client configuration.
func (h *PlexHandler) UpdatePlexConfig(c *fiber.Ctx) error {
	var config models.PlexConfig
	if err := c.BodyParser(&config); err != nil {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Invalid request body: " + err.Error(),
		})
	}

	if err := h.repo.UpdatePlexConfig(&config); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to update Plex configuration: " + err.Error(),
		})
	}

	return c.JSON(config)
}
