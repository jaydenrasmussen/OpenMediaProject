package db

import (
	"encoding/json"

	"backend/models"

	"github.com/xujiajun/nutsdb"
)

// PlexRepository handles the database operations for the Plex client configuration.
type PlexRepository struct {
	db *nutsdb.DB
}

// NewPlexRepository creates a new PlexRepository.
func NewPlexRepository(db *nutsdb.DB) *PlexRepository {
	return &PlexRepository{db: db}
}

// GetPlexConfig retrieves the Plex client configuration from the database.
func (r *PlexRepository) GetPlexConfig() (*models.PlexConfig, error) {
	var config models.PlexConfig
	err := r.db.View(func(tx *nutsdb.Tx) error {
		entry, err := tx.Get(PlexBucket, []byte("config"))
		if err != nil {
			return err
		}

		return json.Unmarshal(entry.Value, &config)
	})

	if err != nil {
		return nil, err
	}

	return &config, nil
}

// UpdatePlexConfig updates the Plex client configuration in the database.
func (r *PlexRepository) UpdatePlexConfig(config *models.PlexConfig) error {
	return r.db.Update(func(tx *nutsdb.Tx) error {
		data, err := json.Marshal(config)
		if err != nil {
			return err
		}

		return tx.Put(PlexBucket, []byte("config"), data, 0)
	})
}
