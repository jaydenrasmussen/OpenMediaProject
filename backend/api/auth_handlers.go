package api

import (
	"crypto/subtle"
	"time"

	"backend/config"
	"backend/middleware"

	"github.com/gofiber/fiber/v2"
)

// APIKeyResponse represents the response for the API key retrieval/generation
type APIKeyResponse struct {
	APIKey      string    `json:"api_key,omitempty"`
	Generated   bool      `json:"generated"`
	ExpiresAt   time.Time `json:"expires_at,omitempty"`
	Environment string    `json:"environment"`
}

// GetAPIKey returns the current API key if auth is enabled
func GetAPIKey(c *fiber.Ctx) error {
	// Load configuration
	cfg, err := config.Load()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to load configuration: " + err.Error(),
		})
	}

	// Check if auth is enabled
	if !cfg.Auth.EnableAuth {
		return c.Status(fiber.StatusOK).JSON(fiber.Map{
			"enabled": false,
			"message": "API key authentication is not enabled",
		})
	}

	// Check if the API key exists
	if cfg.Auth.APIKey == "" {
		return c.Status(fiber.StatusNotFound).JSON(fiber.Map{
			"error":   true,
			"message": "No API key has been generated yet",
		})
	}

	// Return the API key information
	// Note: In production, you might want to limit this endpoint to admin users
	// or require additional authentication
	return c.JSON(APIKeyResponse{
		APIKey:      cfg.Auth.APIKey,
		Generated:   true,
		Environment: cfg.Environment,
	})
}

// GenerateNewAPIKey generates a new API key
func GenerateNewAPIKey(c *fiber.Ctx) error {
	// Load configuration
	cfg, err := config.Load()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to load configuration: " + err.Error(),
		})
	}

	// Check if auth is enabled
	if !cfg.Auth.EnableAuth {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "API key authentication is not enabled. Enable it in the configuration first.",
		})
	}

	// Generate a new API key
	apiKey, err := middleware.GenerateAPIKey(32)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to generate API key: " + err.Error(),
		})
	}

	// Update the configuration
	// Note: In a real-world application, you should persist this to a
	// database or environment variable for future server restarts
	cfg.Auth.APIKey = apiKey

	// Return the new API key
	return c.JSON(APIKeyResponse{
		APIKey:      apiKey,
		Generated:   true,
		Environment: cfg.Environment,
		// Note: You might want to add an expiry date for the API key
		// ExpiresAt: time.Now().Add(30 * 24 * time.Hour), // 30 days
	})
}

// ValidateAPIKey validates the provided API key
func ValidateAPIKey(c *fiber.Ctx) error {
	// Load configuration
	cfg, err := config.Load()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to load configuration: " + err.Error(),
		})
	}

	// Check if auth is enabled
	if !cfg.Auth.EnableAuth {
		return c.Status(fiber.StatusOK).JSON(fiber.Map{
			"valid":     true,
			"message":   "API key authentication is not enabled, any key is valid",
			"auth_mode": "disabled",
		})
	}

	// Get the API key from the request
	providedKey := c.Get(middleware.DefaultAPIKeyHeader)
	if providedKey == "" && cfg.Auth.AllowQueryParam {
		providedKey = c.Query("api_key")
	}

	// Check if the API key is valid
	isValid := cfg.Auth.APIKey != "" && providedKey != "" &&
		subtle.ConstantTimeCompare([]byte(providedKey), []byte(cfg.Auth.APIKey)) == 1

	if isValid {
		return c.Status(fiber.StatusOK).JSON(fiber.Map{
			"valid":     true,
			"message":   "API key is valid",
			"auth_mode": "enabled",
		})
	}

	return c.Status(fiber.StatusUnauthorized).JSON(fiber.Map{
		"valid":     false,
		"message":   "Invalid API key",
		"auth_mode": "enabled",
	})
}
