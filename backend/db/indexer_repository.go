package db

import (
	"encoding/json"
	"errors"
	"time"

	"backend/models"

	"github.com/jaevor/go-nanoid"
	"github.com/xujiajun/nutsdb"
)

var ErrIndexerNotFound = errors.New("indexer not found")

// IndexerRepository handles indexer data operations with NutsDB
type IndexerRepository struct {
	db *nutsdb.DB
}

// NewIndexerRepository creates a new IndexerRepository
func NewIndexerRepository() *IndexerRepository {
	return &IndexerRepository{
		db: GetDB(),
	}
}

// GetAll retrieves all indexers
func (r *IndexerRepository) GetAll() ([]*models.Indexer, error) {
	indexers := make([]*models.Indexer, 0)

	err := r.db.View(func(tx *nutsdb.Tx) error {
		entries, err := tx.GetAll(IndexerBucket)
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

			var indexer models.Indexer
			if err := json.Unmarshal(entry.Value, &indexer); err != nil {
				return err
			}
			indexers = append(indexers, &indexer)
		}

		return nil
	})

	if err != nil {
		return nil, err
	}

	return indexers, nil
}

// GetByID retrieves an indexer by ID
func (r *IndexerRepository) GetByID(id string) (*models.Indexer, error) {
	var indexer models.Indexer

	err := r.db.View(func(tx *nutsdb.Tx) error {
		entry, err := tx.Get(IndexerBucket, []byte(id))
		if err != nil {
			if err == nutsdb.ErrKeyNotFound {
				return ErrIndexerNotFound
			}
			return err
		}

		return json.Unmarshal(entry.Value, &indexer)
	})

	if err != nil {
		return nil, err
	}

	return &indexer, nil
}

// Save upserts an indexer. If ID is empty, a new nanoid is generated.
func (r *IndexerRepository) Save(indexer *models.Indexer) error {
	if indexer.ID == "" {
		generate, err := nanoid.Standard(21)
		if err != nil {
			return err
		}
		indexer.ID = generate()
	}

	if indexer.AddedAt.IsZero() {
		indexer.AddedAt = time.Now()
	}

	data, err := json.Marshal(indexer)
	if err != nil {
		return err
	}

	return r.db.Update(func(tx *nutsdb.Tx) error {
		return tx.Put(IndexerBucket, []byte(indexer.ID), data, 0)
	})
}

// Delete removes an indexer by ID
func (r *IndexerRepository) Delete(id string) error {
	return r.db.Update(func(tx *nutsdb.Tx) error {
		return tx.Delete(IndexerBucket, []byte(id))
	})
}
