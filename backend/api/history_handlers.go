package api

import (
	"backend/db"

	"github.com/gofiber/fiber/v2"
)

// HistoryHandler handles activity history endpoints.
type HistoryHandler struct{}

// NewHistoryHandler creates a new HistoryHandler.
func NewHistoryHandler() *HistoryHandler {
	return &HistoryHandler{}
}

// GetHistory handles GET /api/v1/history.
func (h *HistoryHandler) GetHistory(c *fiber.Ctx) error {
	repo := db.NewHistoryRepository()
	entries, err := repo.GetAll()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to load history: " + err.Error(),
		})
	}

	return c.JSON(fiber.Map{
		"entries": entries,
		"count":   len(entries),
	})
}
