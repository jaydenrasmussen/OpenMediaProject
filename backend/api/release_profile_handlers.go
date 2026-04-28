package api

import (
	"backend/db"
	"backend/models"

	"github.com/gofiber/fiber/v2"
)

// releaseProfileBody is used to parse create/update request bodies.
type releaseProfileBody struct {
	Name    string   `json:"name"`
	Require []string `json:"require"`
	Reject  []string `json:"reject"`
	Prefer  []string `json:"prefer"`
	Enabled bool     `json:"enabled"`
}

// ListReleaseProfiles handles GET /api/v1/release-profiles.
func ListReleaseProfiles(c *fiber.Ctx) error {
	profiles, err := db.NewReleaseProfileRepository().GetAll()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to retrieve release profiles: " + err.Error(),
		})
	}
	return c.JSON(fiber.Map{"profiles": profiles, "count": len(profiles)})
}

// CreateReleaseProfile handles POST /api/v1/release-profiles.
func CreateReleaseProfile(c *fiber.Ctx) error {
	var body releaseProfileBody
	if err := c.BodyParser(&body); err != nil {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Invalid request body: " + err.Error(),
		})
	}
	if body.Name == "" {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "name is required",
		})
	}

	p := &models.ReleaseProfile{
		Name:    body.Name,
		Require: body.Require,
		Reject:  body.Reject,
		Prefer:  body.Prefer,
		Enabled: body.Enabled,
	}
	if err := db.NewReleaseProfileRepository().Save(p); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to save release profile: " + err.Error(),
		})
	}
	return c.Status(fiber.StatusCreated).JSON(p)
}

// UpdateReleaseProfile handles PUT /api/v1/release-profiles/:id.
func UpdateReleaseProfile(c *fiber.Ctx) error {
	id := c.Params("id")
	repo := db.NewReleaseProfileRepository()

	existing, err := repo.GetByID(id)
	if err != nil {
		if err == db.ErrReleaseProfileNotFound {
			return c.Status(fiber.StatusNotFound).JSON(fiber.Map{
				"error":   true,
				"message": "Release profile not found",
			})
		}
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to retrieve release profile: " + err.Error(),
		})
	}

	var body releaseProfileBody
	if err := c.BodyParser(&body); err != nil {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Invalid request body: " + err.Error(),
		})
	}

	if body.Name != "" {
		existing.Name = body.Name
	}
	existing.Require = body.Require
	existing.Reject = body.Reject
	existing.Prefer = body.Prefer
	existing.Enabled = body.Enabled

	if err := repo.Save(existing); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to update release profile: " + err.Error(),
		})
	}
	return c.JSON(existing)
}

// DeleteReleaseProfile handles DELETE /api/v1/release-profiles/:id.
func DeleteReleaseProfile(c *fiber.Ctx) error {
	id := c.Params("id")
	repo := db.NewReleaseProfileRepository()

	if _, err := repo.GetByID(id); err != nil {
		if err == db.ErrReleaseProfileNotFound {
			return c.Status(fiber.StatusNotFound).JSON(fiber.Map{
				"error":   true,
				"message": "Release profile not found",
			})
		}
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to retrieve release profile: " + err.Error(),
		})
	}

	if err := repo.Delete(id); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to delete release profile: " + err.Error(),
		})
	}
	return c.SendStatus(fiber.StatusNoContent)
}
