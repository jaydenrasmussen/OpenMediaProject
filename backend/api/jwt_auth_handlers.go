package api

import (
	"crypto/rand"
	"os"
	"time"

	"backend/db"
	"backend/models"

	"github.com/golang-jwt/jwt/v5"
	"github.com/gofiber/fiber/v2"
	"github.com/google/uuid"
	"golang.org/x/crypto/bcrypt"
)

// JWTSecret is the HMAC signing key for session JWTs.
// Loaded from JWT_SECRET env var; falls back to a stable .apikey file value,
// or a random per-process value (sessions lost on restart if neither is set).
var JWTSecret = func() []byte {
	if s := os.Getenv("JWT_SECRET"); s != "" {
		return []byte(s)
	}
	if data, err := os.ReadFile(".apikey"); err == nil && len(data) >= 16 {
		return data
	}
	b := make([]byte, 32)
	_, _ = rand.Read(b)
	return b
}()

// jwtSecret is an unexported alias used within this package.
var jwtSecret = JWTSecret

// JWTClaims holds the payload for session JWTs.
type JWTClaims struct {
	UserID   string          `json:"uid"`
	Username string          `json:"sub"`
	Role     models.UserRole `json:"role"`
	jwt.RegisteredClaims
}

// LoginRequest is the body for POST /api/v1/auth/login.
type LoginRequest struct {
	Username string `json:"username"`
	Password string `json:"password"`
}

// LoginResponse is returned on successful login.
type LoginResponse struct {
	Token    string `json:"token"`
	UserID   string `json:"user_id"`
	Username string `json:"username"`
	Role     string `json:"role"`
}

// Login handles POST /api/v1/auth/login.
func Login(c *fiber.Ctx) error {
	var req LoginRequest
	if err := c.BodyParser(&req); err != nil {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Invalid request body",
		})
	}

	repo := db.NewUserRepository()
	user, err := repo.GetByUsername(req.Username)
	if err != nil {
		return c.Status(fiber.StatusUnauthorized).JSON(fiber.Map{
			"error":   true,
			"message": "Invalid username or password",
		})
	}

	if !user.IsActive {
		return c.Status(fiber.StatusUnauthorized).JSON(fiber.Map{
			"error":   true,
			"message": "Account is inactive",
		})
	}

	if err := bcrypt.CompareHashAndPassword([]byte(user.PasswordHash), []byte(req.Password)); err != nil {
		return c.Status(fiber.StatusUnauthorized).JSON(fiber.Map{
			"error":   true,
			"message": "Invalid username or password",
		})
	}

	token, err := issueJWT(user)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to issue token",
		})
	}

	return c.JSON(LoginResponse{
		Token:    token,
		UserID:   user.ID,
		Username: user.Username,
		Role:     string(user.Role),
	})
}

// issueJWT creates a signed JWT for the given user, valid for 24 hours.
func issueJWT(user *models.User) (string, error) {
	claims := JWTClaims{
		UserID:   user.ID,
		Username: user.Username,
		Role:     user.Role,
		RegisteredClaims: jwt.RegisteredClaims{
			ID:        uuid.NewString(),
			IssuedAt:  jwt.NewNumericDate(time.Now()),
			ExpiresAt: jwt.NewNumericDate(time.Now().Add(24 * time.Hour)),
		},
	}
	t := jwt.NewWithClaims(jwt.SigningMethodHS256, claims)
	return t.SignedString(jwtSecret)
}

// ParseJWT validates a JWT string and returns its claims.
func ParseJWT(tokenStr string) (*JWTClaims, error) {
	claims := &JWTClaims{}
	_, err := jwt.ParseWithClaims(tokenStr, claims, func(t *jwt.Token) (interface{}, error) {
		if _, ok := t.Method.(*jwt.SigningMethodHMAC); !ok {
			return nil, jwt.ErrSignatureInvalid
		}
		return jwtSecret, nil
	})
	if err != nil {
		return nil, err
	}
	return claims, nil
}

// RegisterUser handles POST /api/v1/auth/register.
// Creates a new user with a bcrypt-hashed password.
func RegisterUser(c *fiber.Ctx) error {
	var body struct {
		Username  string `json:"username"`
		Email     string `json:"email"`
		Password  string `json:"password"`
		FirstName string `json:"first_name"`
		LastName  string `json:"last_name"`
	}
	if err := c.BodyParser(&body); err != nil {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Invalid request body",
		})
	}
	if body.Username == "" || body.Password == "" {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "username and password are required",
		})
	}

	hash, err := bcrypt.GenerateFromPassword([]byte(body.Password), bcrypt.DefaultCost)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to hash password",
		})
	}

	user := &models.User{
		ID:           uuid.NewString(),
		Username:     body.Username,
		Email:        body.Email,
		PasswordHash: string(hash),
		FirstName:    body.FirstName,
		LastName:     body.LastName,
	}
	user.BeforeCreate()

	repo := db.NewUserRepository()
	if err := repo.Create(user); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to create user: " + err.Error(),
		})
	}

	return c.Status(fiber.StatusCreated).JSON(fiber.Map{
		"id":       user.ID,
		"username": user.Username,
		"email":    user.Email,
		"role":     user.Role,
	})
}
