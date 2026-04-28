package middleware

import (
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/gofiber/fiber/v2"
)

// ── GenerateAPIKey ────────────────────────────────────────────────────────────

func TestGenerateAPIKey_DefaultLength(t *testing.T) {
	key, err := GenerateAPIKey()
	if err != nil {
		t.Fatalf("GenerateAPIKey() error = %v", err)
	}
	if len(key) != 32 {
		t.Errorf("len(key) = %d, want 32", len(key))
	}
}

func TestGenerateAPIKey_CustomLengths(t *testing.T) {
	for _, length := range []int{16, 24, 48, 64} {
		key, err := GenerateAPIKey(length)
		if err != nil {
			t.Fatalf("GenerateAPIKey(%d) error = %v", length, err)
		}
		if len(key) != length {
			t.Errorf("GenerateAPIKey(%d): len = %d, want %d", length, len(key), length)
		}
	}
}

func TestGenerateAPIKey_ZeroLengthFallsBackToDefault(t *testing.T) {
	// Passing 0 should fall back to the internal default of 32.
	key, err := GenerateAPIKey(0)
	if err != nil {
		t.Fatalf("GenerateAPIKey(0) error = %v", err)
	}
	if len(key) != 32 {
		t.Errorf("len(key) = %d, want 32 (zero should use default)", len(key))
	}
}

func TestGenerateAPIKey_IsUnique(t *testing.T) {
	seen := make(map[string]bool, 50)
	for i := 0; i < 50; i++ {
		key, err := GenerateAPIKey()
		if err != nil {
			t.Fatalf("GenerateAPIKey() error = %v", err)
		}
		if seen[key] {
			t.Fatalf("duplicate key generated: %q", key)
		}
		seen[key] = true
	}
}

// ── GenerateCaseInsensitiveAPIKey ─────────────────────────────────────────────

func TestGenerateCaseInsensitiveAPIKey_Length(t *testing.T) {
	for _, length := range []int{16, 32, 64} {
		key, err := GenerateCaseInsensitiveAPIKey(length)
		if err != nil {
			t.Fatalf("GenerateCaseInsensitiveAPIKey(%d) error = %v", length, err)
		}
		if len(key) != length {
			t.Errorf("GenerateCaseInsensitiveAPIKey(%d): len = %d, want %d", length, len(key), length)
		}
	}
}

func TestGenerateCaseInsensitiveAPIKey_CharacterSet(t *testing.T) {
	key, err := GenerateCaseInsensitiveAPIKey(64)
	if err != nil {
		t.Fatalf("GenerateCaseInsensitiveAPIKey(64) error = %v", err)
	}
	for i, c := range key {
		if !((c >= 'a' && c <= 'z') || (c >= '0' && c <= '9')) {
			t.Errorf("key[%d] = %q is not lowercase alphanumeric", i, c)
		}
	}
}

func TestGenerateCaseInsensitiveAPIKey_IsUnique(t *testing.T) {
	seen := make(map[string]bool, 50)
	for i := 0; i < 50; i++ {
		key, err := GenerateCaseInsensitiveAPIKey(32)
		if err != nil {
			t.Fatalf("GenerateCaseInsensitiveAPIKey(32) error = %v", err)
		}
		if seen[key] {
			t.Fatalf("duplicate key generated: %q", key)
		}
		seen[key] = true
	}
}

// ── NewAPIKeyAuthMiddleware ───────────────────────────────────────────────────

// testApp builds a minimal Fiber app with the API key middleware and a single
// GET /test route that returns 200.
func testApp(cfg AuthConfig) *fiber.App {
	app := fiber.New()
	app.Use(NewAPIKeyAuthMiddleware(cfg))
	app.Get("/test", func(c *fiber.Ctx) error {
		return c.SendStatus(fiber.StatusOK)
	})
	return app
}

func TestAPIKeyMiddleware_ValidKey(t *testing.T) {
	cfg := AuthConfig{
		APIKey:         "secret-key",
		Header:         DefaultAPIKeyHeader,
		SuccessHandler: func(c *fiber.Ctx) error { return c.Next() },
		ErrorHandler:   ConfigDefault.ErrorHandler,
	}
	app := testApp(cfg)

	req := httptest.NewRequest(http.MethodGet, "/test", nil)
	req.Header.Set(DefaultAPIKeyHeader, "secret-key")

	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error = %v", err)
	}
	if resp.StatusCode != http.StatusOK {
		t.Errorf("status = %d, want %d", resp.StatusCode, http.StatusOK)
	}
}

func TestAPIKeyMiddleware_ValidKeyCaseInsensitive(t *testing.T) {
	cfg := AuthConfig{
		APIKey:         "SECRET-KEY",
		Header:         DefaultAPIKeyHeader,
		SuccessHandler: func(c *fiber.Ctx) error { return c.Next() },
		ErrorHandler:   ConfigDefault.ErrorHandler,
	}
	app := testApp(cfg)

	req := httptest.NewRequest(http.MethodGet, "/test", nil)
	req.Header.Set(DefaultAPIKeyHeader, "secret-key") // lowercase should still match

	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error = %v", err)
	}
	if resp.StatusCode != http.StatusOK {
		t.Errorf("status = %d, want %d (key comparison must be case-insensitive)", resp.StatusCode, http.StatusOK)
	}
}

func TestAPIKeyMiddleware_MissingKey(t *testing.T) {
	cfg := AuthConfig{
		APIKey:         "secret-key",
		Header:         DefaultAPIKeyHeader,
		SuccessHandler: func(c *fiber.Ctx) error { return c.Next() },
		ErrorHandler:   ConfigDefault.ErrorHandler,
	}
	app := testApp(cfg)

	req := httptest.NewRequest(http.MethodGet, "/test", nil)
	// No X-API-Key header.

	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error = %v", err)
	}
	if resp.StatusCode != http.StatusUnauthorized {
		t.Errorf("status = %d, want %d", resp.StatusCode, http.StatusUnauthorized)
	}
}

func TestAPIKeyMiddleware_WrongKey(t *testing.T) {
	cfg := AuthConfig{
		APIKey:         "correct-key",
		Header:         DefaultAPIKeyHeader,
		SuccessHandler: func(c *fiber.Ctx) error { return c.Next() },
		ErrorHandler:   ConfigDefault.ErrorHandler,
	}
	app := testApp(cfg)

	req := httptest.NewRequest(http.MethodGet, "/test", nil)
	req.Header.Set(DefaultAPIKeyHeader, "wrong-key")

	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error = %v", err)
	}
	if resp.StatusCode != http.StatusUnauthorized {
		t.Errorf("status = %d, want %d", resp.StatusCode, http.StatusUnauthorized)
	}
}

func TestAPIKeyMiddleware_SkipEndpoint(t *testing.T) {
	cfg := AuthConfig{
		APIKey:         "secret-key",
		Header:         DefaultAPIKeyHeader,
		SkipEndpoints:  []string{"/api/health"},
		SuccessHandler: func(c *fiber.Ctx) error { return c.Next() },
		ErrorHandler:   ConfigDefault.ErrorHandler,
	}
	app := fiber.New()
	app.Use(NewAPIKeyAuthMiddleware(cfg))
	app.Get("/api/health", func(c *fiber.Ctx) error { return c.SendStatus(fiber.StatusOK) })

	req := httptest.NewRequest(http.MethodGet, "/api/health", nil)
	// No API key — should be skipped.

	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error = %v", err)
	}
	if resp.StatusCode != http.StatusOK {
		t.Errorf("status = %d, want %d (skipped endpoint should not require auth)", resp.StatusCode, http.StatusOK)
	}
}

func TestAPIKeyMiddleware_AllowQueryParam(t *testing.T) {
	cfg := AuthConfig{
		APIKey:         "qparam-key",
		Header:         DefaultAPIKeyHeader,
		AllowQuery:     true,
		SuccessHandler: func(c *fiber.Ctx) error { return c.Next() },
		ErrorHandler:   ConfigDefault.ErrorHandler,
	}
	app := testApp(cfg)

	req := httptest.NewRequest(http.MethodGet, "/test?api_key=qparam-key", nil)

	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error = %v", err)
	}
	if resp.StatusCode != http.StatusOK {
		t.Errorf("status = %d, want %d (query param auth must work when AllowQuery=true)", resp.StatusCode, http.StatusOK)
	}
}

func TestAPIKeyMiddleware_QueryParamDisallowed(t *testing.T) {
	cfg := AuthConfig{
		APIKey:         "qparam-key",
		Header:         DefaultAPIKeyHeader,
		AllowQuery:     false, // explicitly disabled
		SuccessHandler: func(c *fiber.Ctx) error { return c.Next() },
		ErrorHandler:   ConfigDefault.ErrorHandler,
	}
	app := testApp(cfg)

	req := httptest.NewRequest(http.MethodGet, "/test?api_key=qparam-key", nil)

	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error = %v", err)
	}
	if resp.StatusCode != http.StatusUnauthorized {
		t.Errorf("status = %d, want %d (query param auth must be rejected when AllowQuery=false)", resp.StatusCode, http.StatusUnauthorized)
	}
}

func TestAPIKeyMiddleware_ExpiredKey(t *testing.T) {
	past := time.Now().Add(-time.Hour)
	cfg := AuthConfig{
		APIKey:         "expired-key",
		Header:         DefaultAPIKeyHeader,
		ExpiryTime:     &past,
		SuccessHandler: func(c *fiber.Ctx) error { return c.Next() },
		ErrorHandler:   ConfigDefault.ErrorHandler,
	}
	app := testApp(cfg)

	req := httptest.NewRequest(http.MethodGet, "/test", nil)
	req.Header.Set(DefaultAPIKeyHeader, "expired-key")

	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error = %v", err)
	}
	if resp.StatusCode != http.StatusUnauthorized {
		t.Errorf("status = %d, want %d (expired key must be rejected)", resp.StatusCode, http.StatusUnauthorized)
	}
}

func TestAPIKeyMiddleware_NonExpiredKey(t *testing.T) {
	future := time.Now().Add(time.Hour)
	cfg := AuthConfig{
		APIKey:         "valid-key",
		Header:         DefaultAPIKeyHeader,
		ExpiryTime:     &future,
		SuccessHandler: func(c *fiber.Ctx) error { return c.Next() },
		ErrorHandler:   ConfigDefault.ErrorHandler,
	}
	app := testApp(cfg)

	req := httptest.NewRequest(http.MethodGet, "/test", nil)
	req.Header.Set(DefaultAPIKeyHeader, "valid-key")

	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error = %v", err)
	}
	if resp.StatusCode != http.StatusOK {
		t.Errorf("status = %d, want %d (non-expired key must be accepted)", resp.StatusCode, http.StatusOK)
	}
}

func TestAPIKeyMiddleware_DefaultConfig(t *testing.T) {
	// NewAPIKeyAuthMiddleware() with no arguments uses ConfigDefault.
	app := fiber.New()
	app.Use(NewAPIKeyAuthMiddleware())
	app.Get("/test", func(c *fiber.Ctx) error { return c.SendStatus(fiber.StatusOK) })

	req := httptest.NewRequest(http.MethodGet, "/test", nil)
	req.Header.Set(DefaultAPIKeyHeader, "anything") // any key → default has no key set → rejected

	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error = %v", err)
	}
	// Default config has no API key configured → any key fails constant-time compare.
	if resp.StatusCode == http.StatusOK {
		t.Errorf("expected non-200 for default (unconfigured) middleware")
	}
}

func TestAPIKeyMiddleware_CustomHeader(t *testing.T) {
	const customHeader = "X-Custom-Auth"
	cfg := AuthConfig{
		APIKey:         "my-key",
		Header:         customHeader,
		SuccessHandler: func(c *fiber.Ctx) error { return c.Next() },
		ErrorHandler:   ConfigDefault.ErrorHandler,
	}
	app := testApp(cfg)

	req := httptest.NewRequest(http.MethodGet, "/test", nil)
	req.Header.Set(customHeader, "my-key")

	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error = %v", err)
	}
	if resp.StatusCode != http.StatusOK {
		t.Errorf("status = %d, want %d (custom header auth)", resp.StatusCode, http.StatusOK)
	}
}

func TestAPIKeyMiddleware_SkipEndpointPrefix(t *testing.T) {
	cfg := AuthConfig{
		APIKey:         "secret",
		Header:         DefaultAPIKeyHeader,
		SkipEndpoints:  []string{"/api/v1/auth"},
		SuccessHandler: func(c *fiber.Ctx) error { return c.Next() },
		ErrorHandler:   ConfigDefault.ErrorHandler,
	}
	app := fiber.New()
	app.Use(NewAPIKeyAuthMiddleware(cfg))
	app.Post("/api/v1/auth/login", func(c *fiber.Ctx) error { return c.SendStatus(fiber.StatusOK) })

	req := httptest.NewRequest(http.MethodPost, "/api/v1/auth/login", strings.NewReader("{}"))
	// No API key — prefix /api/v1/auth should match.

	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error = %v", err)
	}
	if resp.StatusCode != http.StatusOK {
		t.Errorf("status = %d, want %d (prefix skip should allow sub-paths)", resp.StatusCode, http.StatusOK)
	}
}
