package db

import (
	"encoding/json"
	"errors"
	"time"

	"backend/models"

	"github.com/xujiajun/nutsdb"
)

var (
	ErrTVShowNotFound = errors.New("tv show not found")
	ErrTVShowExists   = errors.New("tv show with this ID already exists")
)

// TVShowRepository handles TV show data operations with NutsDB
type TVShowRepository struct {
	db *nutsdb.DB
}

// NewTVShowRepository creates a new TV show repository
func NewTVShowRepository() *TVShowRepository {
	return &TVShowRepository{
		db: GetDB(),
	}
}

// Create adds a new TV show to the database
func (r *TVShowRepository) Create(show *models.TVShow) error {
	// Convert TV show to JSON
	showData, err := json.Marshal(show)
	if err != nil {
		return err
	}

	// Store in database
	return r.db.Update(func(tx *nutsdb.Tx) error {
		// Check if key already exists
		_, err := tx.Get(TVShowBucket, []byte(show.ID))
		if err == nil {
			return ErrTVShowExists
		}
		if err != nutsdb.ErrKeyNotFound {
			return err
		}

		// Store TV show data
		return tx.Put(TVShowBucket, []byte(show.ID), showData, 0) // 0 means no expiration
	})
}

// Update updates an existing TV show
func (r *TVShowRepository) Update(show *models.TVShow) error {
	// Convert to JSON
	showData, err := json.Marshal(show)
	if err != nil {
		return err
	}

	return r.db.Update(func(tx *nutsdb.Tx) error {
		// Check if TV show exists
		_, err := tx.Get(TVShowBucket, []byte(show.ID))
		if err != nil {
			if err == nutsdb.ErrKeyNotFound {
				return ErrTVShowNotFound
			}
			return err
		}

		// Update TV show
		return tx.Put(TVShowBucket, []byte(show.ID), showData, 0)
	})
}

// GetByID retrieves a TV show by ID
func (r *TVShowRepository) GetByID(id string) (*models.TVShow, error) {
	var show models.TVShow

	err := r.db.View(func(tx *nutsdb.Tx) error {
		entry, err := tx.Get(TVShowBucket, []byte(id))
		if err != nil {
			if err == nutsdb.ErrKeyNotFound {
				return ErrTVShowNotFound
			}
			return err
		}

		// Deserialize data
		return json.Unmarshal(entry.Value, &show)
	})

	if err != nil {
		return nil, err
	}

	return &show, nil
}

// GetAll retrieves all TV shows
func (r *TVShowRepository) GetAll() ([]*models.TVShow, error) {
	shows := make([]*models.TVShow, 0)

	err := r.db.View(func(tx *nutsdb.Tx) error {
		entries, err := tx.GetAll(TVShowBucket)
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

			var show models.TVShow
			if err := json.Unmarshal(entry.Value, &show); err != nil {
				return err
			}
			shows = append(shows, &show)
		}

		return nil
	})

	if err != nil {
		return nil, err
	}

	return shows, nil
}

// Delete deletes a TV show by ID
func (r *TVShowRepository) Delete(id string) error {
	return r.db.Update(func(tx *nutsdb.Tx) error {
		return tx.Delete(TVShowBucket, []byte(id))
	})
}

// UpsertBatch upserts multiple TV shows in a single transaction
func (r *TVShowRepository) UpsertBatch(shows []*models.TVShow) error {
	return r.db.Update(func(tx *nutsdb.Tx) error {
		for _, show := range shows {
			// Convert TV show to JSON
			showData, err := json.Marshal(show)
			if err != nil {
				return err
			}

			// Put the TV show in the database
			if err := tx.Put(TVShowBucket, []byte(show.ID), showData, 0); err != nil {
				return err
			}
		}
		return nil
	})
}

// GetRecentlyUpdated retrieves TV shows updated after a certain time
func (r *TVShowRepository) GetRecentlyUpdated(since time.Time) ([]*models.TVShow, error) {
	shows := make([]*models.TVShow, 0)

	err := r.db.View(func(tx *nutsdb.Tx) error {
		entries, err := tx.GetAll(TVShowBucket)
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

			var show models.TVShow
			if err := json.Unmarshal(entry.Value, &show); err != nil {
				return err
			}

			if show.FetchedAt.After(since) {
				shows = append(shows, &show)
			}
		}

		return nil
	})

	if err != nil {
		return nil, err
	}

	return shows, nil
}

// Count returns the number of TV shows in the database
func (r *TVShowRepository) Count() (int, error) {
	var count int

	err := r.db.View(func(tx *nutsdb.Tx) error {
		entries, err := tx.GetAll(TVShowBucket)
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
			count++
		}

		return nil
	})

	if err != nil {
		return 0, err
	}

	return count, nil
}
