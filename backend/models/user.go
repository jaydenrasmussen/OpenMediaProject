package models

import (
	"encoding/json"
	"time"
)

// UserRole defines the access level for a user.
type UserRole string

const (
	UserRoleAdmin UserRole = "admin"
	UserRoleUser  UserRole = "user"
)

// NotificationPreferences holds per-user notification channel toggles.
type NotificationPreferences struct {
	Discord bool `json:"discord"`
	Email   bool `json:"email"`
	Ntfy    bool `json:"ntfy"`
	Gotify  bool `json:"gotify"`
	Apprise bool `json:"apprise"`
}

// User represents a user in the system
type User struct {
	ID            string                  `json:"id"`
	Username      string                  `json:"username"`
	Email         string                  `json:"email"`
	PasswordHash  string                  `json:"-"` // bcrypt hash, never exposed
	Password      string                  `json:"-"` // legacy field, kept for compat
	Role          UserRole                `json:"role"`
	FirstName     string                  `json:"first_name"`
	LastName      string                  `json:"last_name"`
	IsActive      bool                    `json:"is_active"`
	Notifications NotificationPreferences `json:"notifications"`
	CreatedAt     time.Time               `json:"created_at"`
	UpdatedAt     time.Time               `json:"updated_at"`
	DeletedAt     time.Time               `json:"-"` // Soft delete (zero = not deleted)
}

// ToJSON serializes the user to JSON
func (u *User) ToJSON() ([]byte, error) {
	return json.Marshal(u)
}

// FromJSON deserializes the user from JSON
func (u *User) FromJSON(data []byte) error {
	return json.Unmarshal(data, u)
}

// BeforeCreate prepares a user for creation
func (u *User) BeforeCreate() {
	if u.Role == "" {
		u.Role = UserRoleUser
	}
	u.IsActive = true
	now := time.Now()
	u.CreatedAt = now
	u.UpdatedAt = now
}

// GetKey returns the key used to store this user in NutsDB
func (u *User) GetKey() []byte {
	return []byte(u.ID)
}
