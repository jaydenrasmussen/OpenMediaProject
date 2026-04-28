package api

import (
	"backend/db"
	"backend/models"

	"github.com/gofiber/fiber/v2"
	"github.com/google/uuid"
)

// GetUsers returns all users
func GetUsers(c *fiber.Ctx) error {
	userRepo := db.NewUserRepository()
	users, err := userRepo.GetAll()

	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error retrieving users",
		})
	}

	return c.JSON(fiber.Map{
		"users": users,
	})
}

// GetUser returns a specific user by ID
func GetUser(c *fiber.Ctx) error {
	id := c.Params("id")
	userRepo := db.NewUserRepository()

	user, err := userRepo.GetByID(id)
	if err != nil {
		if err == db.ErrUserNotFound {
			return c.Status(fiber.StatusNotFound).JSON(fiber.Map{
				"error":   true,
				"message": "User not found",
			})
		}

		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error retrieving user",
		})
	}

	return c.JSON(user)
}

// CreateUser adds a new user
func CreateUser(c *fiber.Ctx) error {
	user := new(models.User)

	if err := c.BodyParser(user); err != nil {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Invalid request body",
		})
	}

	// Generate a unique ID
	user.ID = uuid.New().String()

	userRepo := db.NewUserRepository()
	if err := userRepo.Create(user); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error creating user: " + err.Error(),
		})
	}

	return c.Status(fiber.StatusCreated).JSON(user)
}

// UpdateUser updates an existing user
func UpdateUser(c *fiber.Ctx) error {
	id := c.Params("id")
	userRepo := db.NewUserRepository()

	// Get existing user first
	existingUser, err := userRepo.GetByID(id)
	if err != nil {
		if err == db.ErrUserNotFound {
			return c.Status(fiber.StatusNotFound).JSON(fiber.Map{
				"error":   true,
				"message": "User not found",
			})
		}

		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error retrieving user",
		})
	}

	// Parse update data
	updatedUser := new(models.User)
	if err := c.BodyParser(updatedUser); err != nil {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Invalid request body",
		})
	}

	// Preserve ID and creation time
	updatedUser.ID = id
	updatedUser.CreatedAt = existingUser.CreatedAt

	// Update user
	if err := userRepo.Update(updatedUser); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error updating user",
		})
	}

	return c.JSON(updatedUser)
}

// DeleteUser removes a user by ID
func DeleteUser(c *fiber.Ctx) error {
	id := c.Params("id")
	userRepo := db.NewUserRepository()

	// Check if user exists
	_, err := userRepo.GetByID(id)
	if err != nil {
		if err == db.ErrUserNotFound {
			return c.Status(fiber.StatusNotFound).JSON(fiber.Map{
				"error":   true,
				"message": "User not found",
			})
		}

		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error retrieving user",
		})
	}

	// Delete the user
	if err := userRepo.Delete(id); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error deleting user",
		})
	}

	return c.SendStatus(fiber.StatusNoContent)
}
