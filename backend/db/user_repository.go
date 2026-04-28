package db

import (
	"encoding/json"
	"errors"
	"time"

	"backend/models"

	"github.com/xujiajun/nutsdb"
)

var (
	ErrUserNotFound = errors.New("user not found")
	ErrKeyExists    = errors.New("user with this key already exists")
)

// UserRepository handles user data operations with NutsDB
type UserRepository struct {
	db *nutsdb.DB
}

// NewUserRepository creates a new user repository
func NewUserRepository() *UserRepository {
	return &UserRepository{
		db: GetDB(),
	}
}

// Create adds a new user to the database
func (r *UserRepository) Create(user *models.User) error {
	// Prepare user for creation (set timestamps, etc.)
	user.BeforeCreate()

	// Convert user to JSON
	userData, err := json.Marshal(user)
	if err != nil {
		return err
	}

	// Store in database
	return r.db.Update(func(tx *nutsdb.Tx) error {
		// Check if key already exists
		_, err := tx.Get(UserBucket, []byte(user.ID))
		if err == nil {
			return ErrKeyExists
		}
		if err != nutsdb.ErrKeyNotFound {
			return err
		}

		// Store user data
		return tx.Put(UserBucket, []byte(user.ID), userData, 0) // 0 means no expiration
	})
}

// GetByID retrieves a user by ID
func (r *UserRepository) GetByID(id string) (*models.User, error) {
	var user models.User

	err := r.db.View(func(tx *nutsdb.Tx) error {
		entry, err := tx.Get(UserBucket, []byte(id))
		if err != nil {
			if err == nutsdb.ErrKeyNotFound {
				return ErrUserNotFound
			}
			return err
		}

		// Deserialize data
		return json.Unmarshal(entry.Value, &user)
	})

	if err != nil {
		return nil, err
	}

	return &user, nil
}

// GetByUsername retrieves a user by username
func (r *UserRepository) GetByUsername(username string) (*models.User, error) {
	var foundUser *models.User

	err := r.db.View(func(tx *nutsdb.Tx) error {
		entries, err := tx.GetAll(UserBucket)
		if err != nil {
			return err
		}

		for _, entry := range entries {
			var user models.User
			if err := json.Unmarshal(entry.Value, &user); err != nil {
				return err
			}

			if user.Username == username {
				foundUser = &user
				return nil
			}
		}

		return ErrUserNotFound
	})

	if err != nil {
		return nil, err
	}

	return foundUser, nil
}

// GetByEmail retrieves a user by email
func (r *UserRepository) GetByEmail(email string) (*models.User, error) {
	var foundUser *models.User

	err := r.db.View(func(tx *nutsdb.Tx) error {
		entries, err := tx.GetAll(UserBucket)
		if err != nil {
			return err
		}

		for _, entry := range entries {
			var user models.User
			if err := json.Unmarshal(entry.Value, &user); err != nil {
				return err
			}

			if user.Email == email {
				foundUser = &user
				return nil
			}
		}

		return ErrUserNotFound
	})

	if err != nil {
		return nil, err
	}

	return foundUser, nil
}

// GetAll retrieves all users
func (r *UserRepository) GetAll() ([]*models.User, error) {
	users := make([]*models.User, 0)

	err := r.db.View(func(tx *nutsdb.Tx) error {
		entries, err := tx.GetAll(UserBucket)
		if err != nil {
			if err == nutsdb.ErrBucketEmpty {
				return nil
			}
			return err
		}

		for _, entry := range entries {
			// Skip initialization entry
			if string(entry.Key) == "_init" {
				continue
			}

			var user models.User
			if err := json.Unmarshal(entry.Value, &user); err != nil {
				return err
			}
			users = append(users, &user)
		}

		return nil
	})

	if err != nil {
		return nil, err
	}

	return users, nil
}

// Update updates an existing user
func (r *UserRepository) Update(user *models.User) error {
	// Update timestamp
	user.UpdatedAt = time.Now()

	// Convert to JSON
	userData, err := json.Marshal(user)
	if err != nil {
		return err
	}

	return r.db.Update(func(tx *nutsdb.Tx) error {
		// Check if user exists
		_, err := tx.Get(UserBucket, []byte(user.ID))
		if err != nil {
			if err == nutsdb.ErrKeyNotFound {
				return ErrUserNotFound
			}
			return err
		}

		// Update user
		return tx.Put(UserBucket, []byte(user.ID), userData, 0)
	})
}

// Delete deletes a user by ID
func (r *UserRepository) Delete(id string) error {
	return r.db.Update(func(tx *nutsdb.Tx) error {
		return tx.Delete(UserBucket, []byte(id))
	})
}

// SoftDelete marks a user as deleted without removing the data
func (r *UserRepository) SoftDelete(id string) error {
	return r.db.Update(func(tx *nutsdb.Tx) error {
		// Get current user data
		entry, err := tx.Get(UserBucket, []byte(id))
		if err != nil {
			if err == nutsdb.ErrKeyNotFound {
				return ErrUserNotFound
			}
			return err
		}

		// Deserialize user
		var user models.User
		if err := json.Unmarshal(entry.Value, &user); err != nil {
			return err
		}

		// Set deletion time
		user.DeletedAt = time.Now()
		user.IsActive = false

		// Save updated user
		userData, err := json.Marshal(user)
		if err != nil {
			return err
		}

		return tx.Put(UserBucket, []byte(id), userData, 0)
	})
}
