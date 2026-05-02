package middleware

import (
	"strings"

	"github.com/gofiber/fiber/v2"
	"github.com/golang-jwt/jwt/v5"
)

const (
	LocalKeyUserID   = "jwt_user_id"
	LocalKeyUsername = "jwt_username"
	LocalKeyRole     = "jwt_role"
)

// JWTMiddleware validates Bearer JWTs on requests.
// It is additive — the existing API key middleware continues to work for
// programmatic access; JWT auth is used for browser sessions.
//
// The middleware is lenient: if no Bearer token is present it simply calls
// Next() and lets downstream handlers decide whether auth is required.
// Handlers that need a verified user should call RequireJWT instead.
func JWTMiddleware(secret []byte) fiber.Handler {
	return func(c *fiber.Ctx) error {
		token := extractBearerToken(c)
		if token == "" {
			return c.Next()
		}

		claims := &struct {
			UserID   string `json:"uid"`
			Username string `json:"sub"`
			Role     string `json:"role"`
			jwt.RegisteredClaims
		}{}

		_, err := jwt.ParseWithClaims(token, claims, func(t *jwt.Token) (interface{}, error) {
			if _, ok := t.Method.(*jwt.SigningMethodHMAC); !ok {
				return nil, jwt.ErrSignatureInvalid
			}
			return secret, nil
		})
		if err != nil {
			// Bad token provided — reject rather than silently skip.
			return c.Status(fiber.StatusUnauthorized).JSON(fiber.Map{
				"error":   true,
				"message": "Invalid or expired token",
			})
		}

		c.Locals(LocalKeyUserID, claims.UserID)
		c.Locals(LocalKeyUsername, claims.Username)
		c.Locals(LocalKeyRole, claims.Role)
		return c.Next()
	}
}

// RequireAuth rejects requests where no valid JWT was parsed.
// Must be placed after JWTMiddleware in the middleware chain.
func RequireAuth(c *fiber.Ctx) error {
	uid, _ := c.Locals(LocalKeyUserID).(string)
	if uid == "" {
		return c.Status(fiber.StatusUnauthorized).JSON(fiber.Map{
			"error":   true,
			"message": "Authentication required",
		})
	}
	return c.Next()
}

// RequireRole is a middleware that rejects requests where the JWT role does
// not match one of the allowed roles. Must be placed after JWTMiddleware.
func RequireRole(roles ...string) fiber.Handler {
	allowed := make(map[string]struct{}, len(roles))
	for _, r := range roles {
		allowed[r] = struct{}{}
	}
	return func(c *fiber.Ctx) error {
		role, _ := c.Locals(LocalKeyRole).(string)
		if _, ok := allowed[role]; !ok {
			return c.Status(fiber.StatusForbidden).JSON(fiber.Map{
				"error":   true,
				"message": "Insufficient permissions",
			})
		}
		return c.Next()
	}
}

func extractBearerToken(c *fiber.Ctx) string {
	auth := c.Get("Authorization")
	if strings.HasPrefix(auth, "Bearer ") {
		return strings.TrimPrefix(auth, "Bearer ")
	}
	return ""
}
