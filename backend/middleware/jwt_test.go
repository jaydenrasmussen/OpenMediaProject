package middleware

import (
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/golang-jwt/jwt/v5"
	"github.com/gofiber/fiber/v2"
)

// ── helpers ───────────────────────────────────────────────────────────────────

var testSecret = []byte("test-jwt-secret-for-unit-tests")

type testClaims struct {
	UserID   string `json:"uid"`
	Username string `json:"sub"`
	Role     string `json:"role"`
	jwt.RegisteredClaims
}

func makeToken(t *testing.T, secret []byte, claims testClaims) string {
	t.Helper()
	token := jwt.NewWithClaims(jwt.SigningMethodHS256, claims)
	signed, err := token.SignedString(secret)
	if err != nil {
		t.Fatalf("jwt.SignedString error = %v", err)
	}
	return signed
}

func jwtApp(secret []byte) *fiber.App {
	app := fiber.New()
	app.Use(JWTMiddleware(secret))
	app.Get("/me", func(c *fiber.Ctx) error {
		uid, _ := c.Locals(LocalKeyUserID).(string)
		user, _ := c.Locals(LocalKeyUsername).(string)
		role, _ := c.Locals(LocalKeyRole).(string)
		return c.JSON(fiber.Map{
			"uid":  uid,
			"user": user,
			"role": role,
		})
	})
	return app
}

// ── JWTMiddleware ─────────────────────────────────────────────────────────────

func TestJWTMiddleware_NoToken_PassesThrough(t *testing.T) {
	app := jwtApp(testSecret)

	req := httptest.NewRequest(http.MethodGet, "/me", nil)
	// No Authorization header.

	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error = %v", err)
	}
	// Middleware is lenient — no token means pass through, not 401.
	if resp.StatusCode != http.StatusOK {
		t.Errorf("status = %d, want %d (no token must pass through)", resp.StatusCode, http.StatusOK)
	}
}

func TestJWTMiddleware_ValidToken_SetsLocals(t *testing.T) {
	app := jwtApp(testSecret)

	token := makeToken(t, testSecret, testClaims{
		UserID:   "user-42",
		Username: "alice",
		Role:     "admin",
		RegisteredClaims: jwt.RegisteredClaims{
			ExpiresAt: jwt.NewNumericDate(time.Now().Add(time.Hour)),
		},
	})

	req := httptest.NewRequest(http.MethodGet, "/me", nil)
	req.Header.Set("Authorization", "Bearer "+token)

	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error = %v", err)
	}
	if resp.StatusCode != http.StatusOK {
		t.Errorf("status = %d, want 200", resp.StatusCode)
	}
}

func TestJWTMiddleware_InvalidToken_Returns401(t *testing.T) {
	app := jwtApp(testSecret)

	req := httptest.NewRequest(http.MethodGet, "/me", nil)
	req.Header.Set("Authorization", "Bearer this.is.not.a.valid.jwt")

	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error = %v", err)
	}
	if resp.StatusCode != http.StatusUnauthorized {
		t.Errorf("status = %d, want %d (invalid token must be rejected)", resp.StatusCode, http.StatusUnauthorized)
	}
}

func TestJWTMiddleware_WrongSecret_Returns401(t *testing.T) {
	app := jwtApp(testSecret)

	// Token signed with a different secret.
	token := makeToken(t, []byte("wrong-secret"), testClaims{
		UserID:   "user-1",
		Username: "bob",
		Role:     "user",
		RegisteredClaims: jwt.RegisteredClaims{
			ExpiresAt: jwt.NewNumericDate(time.Now().Add(time.Hour)),
		},
	})

	req := httptest.NewRequest(http.MethodGet, "/me", nil)
	req.Header.Set("Authorization", "Bearer "+token)

	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error = %v", err)
	}
	if resp.StatusCode != http.StatusUnauthorized {
		t.Errorf("status = %d, want %d (wrong secret must be rejected)", resp.StatusCode, http.StatusUnauthorized)
	}
}

func TestJWTMiddleware_ExpiredToken_Returns401(t *testing.T) {
	app := jwtApp(testSecret)

	token := makeToken(t, testSecret, testClaims{
		UserID:   "user-1",
		Username: "charlie",
		Role:     "user",
		RegisteredClaims: jwt.RegisteredClaims{
			// Expired an hour ago.
			ExpiresAt: jwt.NewNumericDate(time.Now().Add(-time.Hour)),
		},
	})

	req := httptest.NewRequest(http.MethodGet, "/me", nil)
	req.Header.Set("Authorization", "Bearer "+token)

	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error = %v", err)
	}
	if resp.StatusCode != http.StatusUnauthorized {
		t.Errorf("status = %d, want %d (expired token must be rejected)", resp.StatusCode, http.StatusUnauthorized)
	}
}

func TestJWTMiddleware_NonBearerAuth_PassesThrough(t *testing.T) {
	app := jwtApp(testSecret)

	req := httptest.NewRequest(http.MethodGet, "/me", nil)
	req.Header.Set("Authorization", "Basic dXNlcjpwYXNz") // Basic auth, not Bearer.

	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error = %v", err)
	}
	// Non-Bearer auth means no JWT — middleware passes through.
	if resp.StatusCode != http.StatusOK {
		t.Errorf("status = %d, want %d (non-Bearer auth should be ignored by JWT middleware)", resp.StatusCode, http.StatusOK)
	}
}

// ── RequireRole ───────────────────────────────────────────────────────────────

func roleApp(secret []byte, allowedRoles ...string) *fiber.App {
	app := fiber.New()
	app.Use(JWTMiddleware(secret))
	app.Get("/admin", RequireRole(allowedRoles...), func(c *fiber.Ctx) error {
		return c.SendStatus(fiber.StatusOK)
	})
	return app
}

func TestRequireRole_AllowedRole(t *testing.T) {
	app := roleApp(testSecret, "admin")

	token := makeToken(t, testSecret, testClaims{
		Role: "admin",
		RegisteredClaims: jwt.RegisteredClaims{
			ExpiresAt: jwt.NewNumericDate(time.Now().Add(time.Hour)),
		},
	})

	req := httptest.NewRequest(http.MethodGet, "/admin", nil)
	req.Header.Set("Authorization", "Bearer "+token)

	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error = %v", err)
	}
	if resp.StatusCode != http.StatusOK {
		t.Errorf("status = %d, want %d (admin role must be allowed)", resp.StatusCode, http.StatusOK)
	}
}

func TestRequireRole_ForbiddenRole(t *testing.T) {
	app := roleApp(testSecret, "admin")

	token := makeToken(t, testSecret, testClaims{
		Role: "user", // not admin
		RegisteredClaims: jwt.RegisteredClaims{
			ExpiresAt: jwt.NewNumericDate(time.Now().Add(time.Hour)),
		},
	})

	req := httptest.NewRequest(http.MethodGet, "/admin", nil)
	req.Header.Set("Authorization", "Bearer "+token)

	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error = %v", err)
	}
	if resp.StatusCode != http.StatusForbidden {
		t.Errorf("status = %d, want %d (non-admin role must be forbidden)", resp.StatusCode, http.StatusForbidden)
	}
}

func TestRequireRole_MultipleAllowedRoles(t *testing.T) {
	app := roleApp(testSecret, "admin", "moderator")

	for _, role := range []string{"admin", "moderator"} {
		token := makeToken(t, testSecret, testClaims{
			Role: role,
			RegisteredClaims: jwt.RegisteredClaims{
				ExpiresAt: jwt.NewNumericDate(time.Now().Add(time.Hour)),
			},
		})
		req := httptest.NewRequest(http.MethodGet, "/admin", nil)
		req.Header.Set("Authorization", "Bearer "+token)

		resp, err := app.Test(req)
		if err != nil {
			t.Fatalf("app.Test error = %v", err)
		}
		if resp.StatusCode != http.StatusOK {
			t.Errorf("role %q: status = %d, want %d", role, resp.StatusCode, http.StatusOK)
		}
	}
}

func TestRequireRole_NoJWTLocals_Forbidden(t *testing.T) {
	// No JWT middleware in chain → no locals → role is empty → forbidden.
	app := fiber.New()
	app.Get("/admin", RequireRole("admin"), func(c *fiber.Ctx) error {
		return c.SendStatus(fiber.StatusOK)
	})

	req := httptest.NewRequest(http.MethodGet, "/admin", nil)
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error = %v", err)
	}
	if resp.StatusCode != http.StatusForbidden {
		t.Errorf("status = %d, want %d (no role set must be forbidden)", resp.StatusCode, http.StatusForbidden)
	}
}

// ── extractBearerToken ────────────────────────────────────────────────────────

func TestExtractBearerToken_Present(t *testing.T) {
	app := fiber.New()
	var extracted string
	app.Get("/", func(c *fiber.Ctx) error {
		extracted = extractBearerToken(c)
		return nil
	})

	req := httptest.NewRequest(http.MethodGet, "/", nil)
	req.Header.Set("Authorization", "Bearer my-token-value")
	if _, err := app.Test(req); err != nil {
		t.Fatalf("app.Test error = %v", err)
	}
	if extracted != "my-token-value" {
		t.Errorf("extractBearerToken = %q, want %q", extracted, "my-token-value")
	}
}

func TestExtractBearerToken_Absent(t *testing.T) {
	app := fiber.New()
	var extracted string
	app.Get("/", func(c *fiber.Ctx) error {
		extracted = extractBearerToken(c)
		return nil
	})

	req := httptest.NewRequest(http.MethodGet, "/", nil)
	if _, err := app.Test(req); err != nil {
		t.Fatalf("app.Test error = %v", err)
	}
	if extracted != "" {
		t.Errorf("extractBearerToken = %q, want empty", extracted)
	}
}

func TestExtractBearerToken_NonBearer(t *testing.T) {
	app := fiber.New()
	var extracted string
	app.Get("/", func(c *fiber.Ctx) error {
		extracted = extractBearerToken(c)
		return nil
	})

	req := httptest.NewRequest(http.MethodGet, "/", nil)
	req.Header.Set("Authorization", "Basic dXNlcjpwYXNz")
	if _, err := app.Test(req); err != nil {
		t.Fatalf("app.Test error = %v", err)
	}
	if extracted != "" {
		t.Errorf("extractBearerToken = %q, want empty for non-Bearer", extracted)
	}
}
