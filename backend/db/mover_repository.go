package db

import (
	"encoding/json"

	"backend/models"

	"github.com/xujiajun/nutsdb"
)

// MoverRepository handles the database operations for the mover configuration.
type MoverRepository struct {
	db *nutsdb.DB
}

// NewMoverRepository creates a new MoverRepository.
func NewMoverRepository(db *nutsdb.DB) *MoverRepository {
	return &MoverRepository{db: db}
}

// GetMoverConfig retrieves the mover configuration from the database.
func (r *MoverRepository) GetMoverConfig() (*models.FileMoverConfig, error) {
	var config models.FileMoverConfig
	err := r.db.View(func(tx *nutsdb.Tx) error {
		entry, err := tx.Get(MoverBucket, []byte("config"))
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

// UpdateMoverConfig updates the mover configuration in the database.
func (r *MoverRepository) UpdateMoverConfig(config *models.FileMoverConfig) error {
	return r.db.Update(func(tx *nutsdb.Tx) error {
		data, err := json.Marshal(config)
		if err != nil {
			return err
		}

		return tx.Put(MoverBucket, []byte("config"), data, 0)
	})
}
