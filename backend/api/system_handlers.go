package api

import "github.com/gofiber/fiber/v2"

// SystemHandler serves system-level information.
type SystemHandler struct {
	tracker *TaskTracker
}

// NewSystemHandler creates a new SystemHandler.
func NewSystemHandler(tracker *TaskTracker) *SystemHandler {
	return &SystemHandler{tracker: tracker}
}

// GetTasks handles GET /api/v1/system/tasks.
func (h *SystemHandler) GetTasks(c *fiber.Ctx) error {
	tasks := h.tracker.GetAll()
	return c.JSON(fiber.Map{
		"tasks": tasks,
		"count": len(tasks),
	})
}
