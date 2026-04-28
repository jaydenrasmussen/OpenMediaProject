package db

import (
	"encoding/json"
	"sort"
	"time"

	"backend/models"

	"github.com/jaevor/go-nanoid"
	"github.com/xujiajun/nutsdb"
)

// HistoryRepository handles activity history data operations with NutsDB.
type HistoryRepository struct {
	db *nutsdb.DB
}

// NewHistoryRepository creates a new HistoryRepository.
func NewHistoryRepository() *HistoryRepository {
	return &HistoryRepository{db: GetDB()}
}

// Add records a new history entry, generating an ID and timestamp if absent.
func (r *HistoryRepository) Add(entry *models.HistoryEntry) error {
	if entry.ID == "" {
		generate, err := nanoid.Standard(21)
		if err != nil {
			return err
		}
		entry.ID = generate()
	}
	if entry.CreatedAt.IsZero() {
		entry.CreatedAt = time.Now()
	}

	data, err := json.Marshal(entry)
	if err != nil {
		return err
	}

	return r.db.Update(func(tx *nutsdb.Tx) error {
		return tx.Put(HistoryBucket, entry.GetKey(), data, 0)
	})
}

// GetAll returns all history entries sorted newest-first.
func (r *HistoryRepository) GetAll() ([]*models.HistoryEntry, error) {
	entries := make([]*models.HistoryEntry, 0)

	err := r.db.View(func(tx *nutsdb.Tx) error {
		all, err := tx.GetAll(HistoryBucket)
		if err != nil {
			if err == nutsdb.ErrBucketEmpty {
				return nil
			}
			return err
		}
		for _, e := range all {
			if string(e.Key) == "_init" {
				continue
			}
			var h models.HistoryEntry
			if err := json.Unmarshal(e.Value, &h); err != nil {
				return err
			}
			entries = append(entries, &h)
		}
		return nil
	})

	if err != nil {
		return nil, err
	}

	sort.Slice(entries, func(i, j int) bool {
		return entries[i].CreatedAt.After(entries[j].CreatedAt)
	})
	return entries, nil
}
