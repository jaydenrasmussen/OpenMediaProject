package db

import (
	"encoding/json"
	"errors"
	"time"

	"backend/models"

	"github.com/jaevor/go-nanoid"
	"github.com/xujiajun/nutsdb"
)

var ErrReleaseProfileNotFound = errors.New("release profile not found")

// ReleaseProfileRepository handles release profile data operations with NutsDB.
type ReleaseProfileRepository struct {
	db *nutsdb.DB
}

// NewReleaseProfileRepository creates a new ReleaseProfileRepository.
func NewReleaseProfileRepository() *ReleaseProfileRepository {
	return &ReleaseProfileRepository{db: GetDB()}
}

// GetAll retrieves all release profiles.
func (r *ReleaseProfileRepository) GetAll() ([]*models.ReleaseProfile, error) {
	profiles := make([]*models.ReleaseProfile, 0)

	err := r.db.View(func(tx *nutsdb.Tx) error {
		entries, err := tx.GetAll(ReleaseProfileBucket)
		if err != nil {
			if err == nutsdb.ErrBucketEmpty {
				return nil
			}
			return err
		}

		for _, entry := range entries {
			if string(entry.Key) == "_init" {
				continue
			}
			var p models.ReleaseProfile
			if err := json.Unmarshal(entry.Value, &p); err != nil {
				return err
			}
			profiles = append(profiles, &p)
		}
		return nil
	})

	if err != nil {
		return nil, err
	}
	return profiles, nil
}

// GetEnabled returns only profiles with Enabled == true.
func (r *ReleaseProfileRepository) GetEnabled() ([]*models.ReleaseProfile, error) {
	all, err := r.GetAll()
	if err != nil {
		return nil, err
	}
	out := make([]*models.ReleaseProfile, 0, len(all))
	for _, p := range all {
		if p.Enabled {
			out = append(out, p)
		}
	}
	return out, nil
}

// GetByID retrieves a release profile by ID.
func (r *ReleaseProfileRepository) GetByID(id string) (*models.ReleaseProfile, error) {
	var p models.ReleaseProfile

	err := r.db.View(func(tx *nutsdb.Tx) error {
		entry, err := tx.Get(ReleaseProfileBucket, []byte(id))
		if err != nil {
			if err == nutsdb.ErrKeyNotFound {
				return ErrReleaseProfileNotFound
			}
			return err
		}
		return json.Unmarshal(entry.Value, &p)
	})

	if err != nil {
		return nil, err
	}
	return &p, nil
}

// Save upserts a release profile. Generates a nanoid if ID is empty.
func (r *ReleaseProfileRepository) Save(p *models.ReleaseProfile) error {
	if p.ID == "" {
		generate, err := nanoid.Standard(21)
		if err != nil {
			return err
		}
		p.ID = generate()
	}
	if p.CreatedAt.IsZero() {
		p.CreatedAt = time.Now()
	}

	data, err := json.Marshal(p)
	if err != nil {
		return err
	}

	return r.db.Update(func(tx *nutsdb.Tx) error {
		return tx.Put(ReleaseProfileBucket, []byte(p.ID), data, 0)
	})
}

// Delete removes a release profile by ID.
func (r *ReleaseProfileRepository) Delete(id string) error {
	return r.db.Update(func(tx *nutsdb.Tx) error {
		return tx.Delete(ReleaseProfileBucket, []byte(id))
	})
}
