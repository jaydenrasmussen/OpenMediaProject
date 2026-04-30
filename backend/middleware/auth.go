package middleware

import (
	"crypto/subtle"
	"os"
	"strings"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/jaevor/go-nanoid"
)

// DefaultAPIKeyHeader is the default header name for the API key
const DefaultAPIKeyHeader = "X-API-Key"

// AuthConfig defines the config for the auth middleware
type AuthConfig struct {
	// APIKey is the API key that will be used to authenticate requests
	// If empty, it will be loaded from the API_KEY environment variable
	APIKey string

	// Header is the header key where the API key is expected to be
	// Default: X-API-Key
	Header string

	// AllowQuery determines if the API key can be passed as a query parameter
	// named "api_key" instead of in the header
	// Default: false
	AllowQuery bool

	// SuccessHandler is called when authentication is successful
	SuccessHandler fiber.Handler

	// ErrorHandler is called when authentication fails
	ErrorHandler fiber.ErrorHandler

	// SkipEndpoints is a list of endpoints to skip authentication for
	// Example: ["/api/v1/health", "/api/v1/ping"]
	SkipEndpoints []string

	// ExpiryTime is the duration after which the API key will expire
	// If nil, the API key will never expire
	ExpiryTime *time.Time
}

// ConfigDefault is the default config for auth middleware
var ConfigDefault = AuthConfig{
	Header:     DefaultAPIKeyHeader,
	AllowQuery: false,
	SuccessHandler: func(c *fiber.Ctx) error {
		return c.Next()
	},
	ErrorHandler: func(c *fiber.Ctx, err error) error {
		return c.Status(fiber.StatusUnauthorized).JSON(fiber.Map{
			"error":   true,
			"message": "Unauthorized: Invalid API key",
		})
	},
	SkipEndpoints: []string{},
}

// GenerateAPIKey generates a secure, random API key using nanoid
func GenerateAPIKey(length ...int) (string, error) {
	keyLen := 32
	if len(length) > 0 && length[0] > 0 {
		keyLen = length[0]
	}

	// Create a nanoid generator with the specified length
	generator, err := nanoid.Standard(keyLen)
	if err != nil {
		return "", err
	}

	return generator(), nil
}

// GenerateCaseInsensitiveAPIKey generates a secure, random, case-insensitive API key using nanoid.
func GenerateCaseInsensitiveAPIKey(length int) (string, error) {
	// Define a case-insensitive alphabet
	alphabet := "abcdefghijklmnopqrstuvwxyz0123456789"

	// Create a nanoid generator with the specified length and alphabet
	generator, err := nanoid.Custom(alphabet, length)
	if err != nil {
		return "", err
	}

	return generator(), nil
}

// NewAPIKeyAuthMiddleware creates a new middleware handler
func NewAPIKeyAuthMiddleware(config ...AuthConfig) fiber.Handler {
	// Set default config
	cfg := ConfigDefault

	// Override config if provided
	if len(config) > 0 {
		cfg = config[0]
	}

	// Set default header if not provided
	if cfg.Header == "" {
		cfg.Header = DefaultAPIKeyHeader
	}

	// Load API key from environment if not provided
	if cfg.APIKey == "" {
		cfg.APIKey = os.Getenv("API_KEY")
	}

	// Return middleware handler
	return func(c *fiber.Ctx) error {
		// Check if this endpoint should be skipped
		path := c.Path()
		for _, endpoint := range cfg.SkipEndpoints {
			if strings.HasPrefix(path, endpoint) {
				return cfg.SuccessHandler(c)
			}
		}

		// Get API key from header
		apiKey := c.Get(cfg.Header)

		// If header API key is empty and query params are allowed, check query param
		if apiKey == "" && cfg.AllowQuery {
			apiKey = c.Query("api_key")
		}

		// Verify API key with constant time comparison to prevent timing attacks
		if cfg.APIKey == "" || apiKey == "" || subtle.ConstantTimeCompare([]byte(strings.ToLower(apiKey)), []byte(strings.ToLower(cfg.APIKey))) != 1 {
			return cfg.ErrorHandler(c, fiber.ErrUnauthorized)
		}

		// Check if API key has expired
		if cfg.ExpiryTime != nil && time.Now().After(*cfg.ExpiryTime) {
			return cfg.ErrorHandler(c, fiber.NewError(fiber.StatusUnauthorized, "API key has expired"))
		}

		// API key is valid, proceed
		return cfg.SuccessHandler(c)
	}
}
