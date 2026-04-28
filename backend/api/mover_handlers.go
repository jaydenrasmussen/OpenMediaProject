package api

import (
	"backend/db"
	"backend/models"

	"github.com/gofiber/fiber/v2"
)

// MoverHandler holds the dependencies for the mover handlers.
type MoverHandler struct {
	repo *db.MoverRepository
}

// NewMoverHandler creates a new MoverHandler.
func NewMoverHandler(repo *db.MoverRepository) *MoverHandler {
	return &MoverHandler{repo: repo}
}

// GetMoverConfig retrieves the mover configuration.
func (h *MoverHandler) GetMoverConfig(c *fiber.Ctx) error {
	config, err := h.repo.GetMoverConfig()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to get mover configuration: " + err.Error(),
		})
	}

	return c.JSON(config)
}

// UpdateMoverConfig updates the mover configuration.
func (h *MoverHandler) UpdateMoverConfig(c *fiber.Ctx) error {
	var config models.FileMoverConfig
	if err := c.BodyParser(&config); err != nil {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Invalid request body: " + err.Error(),
		})
	}

	if err := h.repo.UpdateMoverConfig(&config); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to update mover configuration: " + err.Error(),
		})
	}

	return c.JSON(config)
}
