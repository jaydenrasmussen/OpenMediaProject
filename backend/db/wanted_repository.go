package db

import (
	"encoding/json"
	"errors"
	"time"

	"backend/models"

	gonanoid "github.com/jaevor/go-nanoid"
	"github.com/rs/zerolog/log"
	"github.com/xujiajun/nutsdb"
)

var ErrWantedNotFound = errors.New("wanted item not found")

// WantedRepository handles wanted item data operations with NutsDB
type WantedRepository struct {
	db *nutsdb.DB
}

// NewWantedRepository creates a new wanted repository
func NewWantedRepository() *WantedRepository {
	return &WantedRepository{
		db: GetDB(),
	}
}

// Save upserts a WantedItem into the database.
// If item.ID is empty, a new nanoid is generated and assigned.
func (r *WantedRepository) Save(item *models.WantedItem) error {
	if item.ID == "" {
		gen, err := gonanoid.Standard(21)
		if err != nil {
			return err
		}
		item.ID = gen()
	}

	data, err := json.Marshal(item)
	if err != nil {
		return err
	}

	return r.db.Update(func(tx *nutsdb.Tx) error {
		return tx.Put(WantedBucket, []byte(item.ID), data, 0)
	})
}

// GetByID retrieves a WantedItem by its ID.
// Returns ErrWantedNotFound if the item does not exist.
func (r *WantedRepository) GetByID(id string) (*models.WantedItem, error) {
	var item models.WantedItem

	err := r.db.View(func(tx *nutsdb.Tx) error {
		entry, err := tx.Get(WantedBucket, []byte(id))
		if err != nil {
			if err == nutsdb.ErrKeyNotFound {
				return ErrWantedNotFound
			}
			return err
		}
		return json.Unmarshal(entry.Value, &item)
	})

	if err != nil {
		return nil, err
	}

	return &item, nil
}

// GetAll retrieves all WantedItems, skipping the _init key.
func (r *WantedRepository) GetAll() ([]*models.WantedItem, error) {
	items := make([]*models.WantedItem, 0)

	err := r.db.View(func(tx *nutsdb.Tx) error {
		entries, err := tx.GetAll(WantedBucket)
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

			var item models.WantedItem
			if err := json.Unmarshal(entry.Value, &item); err != nil {
				log.Warn().Err(err).Str("key", string(entry.Key)).Msg("Failed to unmarshal wanted item, skipping")
				continue
			}

			items = append(items, &item)
		}

		return nil
	})

	if err != nil {
		return nil, err
	}

	return items, nil
}

// GetByStatus returns all WantedItems with the given status.
func (r *WantedRepository) GetByStatus(status string) ([]*models.WantedItem, error) {
	all, err := r.GetAll()
	if err != nil {
		return nil, err
	}

	filtered := make([]*models.WantedItem, 0)
	for _, item := range all {
		if item.Status == status {
			filtered = append(filtered, item)
		}
	}

	return filtered, nil
}

// GetByMedia returns all WantedItems matching the given mediaType and mediaID.
func (r *WantedRepository) GetByMedia(mediaType, mediaID string) ([]*models.WantedItem, error) {
	all, err := r.GetAll()
	if err != nil {
		return nil, err
	}

	filtered := make([]*models.WantedItem, 0)
	for _, item := range all {
		if item.MediaType == mediaType && item.MediaID == mediaID {
			filtered = append(filtered, item)
		}
	}

	return filtered, nil
}

// Delete removes a WantedItem by ID.
func (r *WantedRepository) Delete(id string) error {
	return r.db.Update(func(tx *nutsdb.Tx) error {
		return tx.Delete(WantedBucket, []byte(id))
	})
}

// UpdateStatus loads the item with the given ID, sets its Status and UpdatedAt, then saves it.
func (r *WantedRepository) UpdateStatus(id, status string) error {
	item, err := r.GetByID(id)
	if err != nil {
		return err
	}

	item.Status = status
	item.UpdatedAt = time.Now()

	return r.Save(item)
}
