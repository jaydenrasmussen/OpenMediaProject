package db

import (
	"encoding/json"
	"errors"
	"time"

	"backend/models"

	"github.com/rs/zerolog/log"
	"github.com/xujiajun/nutsdb"
)

var ErrDownloadNotFound = errors.New("pending download not found")

// DownloadRepository handles pending download data operations with NutsDB
type DownloadRepository struct {
	db *nutsdb.DB
}

// NewDownloadRepository creates a new download repository
func NewDownloadRepository() *DownloadRepository {
	return &DownloadRepository{db: GetDB()}
}

// Save inserts or overwrites a PendingDownload record keyed by its hash
func (r *DownloadRepository) Save(d *models.PendingDownload) error {
	data, err := json.Marshal(d)
	if err != nil {
		return err
	}

	return r.db.Update(func(tx *nutsdb.Tx) error {
		return tx.Put(DownloadBucket, []byte(d.Hash), data, 0)
	})
}

// GetByHash retrieves a single PendingDownload by its torrent hash
func (r *DownloadRepository) GetByHash(hash string) (*models.PendingDownload, error) {
	var d models.PendingDownload

	err := r.db.View(func(tx *nutsdb.Tx) error {
		entry, err := tx.Get(DownloadBucket, []byte(hash))
		if err != nil {
			if err == nutsdb.ErrKeyNotFound {
				return ErrDownloadNotFound
			}
			return err
		}

		return json.Unmarshal(entry.Value, &d)
	})

	if err != nil {
		return nil, err
	}

	return &d, nil
}

// GetAll retrieves all PendingDownload records
func (r *DownloadRepository) GetAll() ([]*models.PendingDownload, error) {
	downloads := make([]*models.PendingDownload, 0)

	err := r.db.View(func(tx *nutsdb.Tx) error {
		entries, err := tx.GetAll(DownloadBucket)
		if err != nil {
			if err == nutsdb.ErrBucketEmpty {
				return nil
			}
			return err
		}

		for _, entry := range entries {
			// Skip initialization marker written by initBuckets()
			if string(entry.Key) == "_init" {
				continue
			}

			var d models.PendingDownload
			if err := json.Unmarshal(entry.Value, &d); err != nil {
				// Skip corrupt entries rather than aborting the entire scan
				log.Warn().Err(err).Str("key", string(entry.Key)).Msg("download_repository: skipping corrupt entry")
				continue
			}
			downloads = append(downloads, &d)
		}

		return nil
	})

	if err != nil {
		return nil, err
	}

	return downloads, nil
}

// GetByStatus returns all PendingDownload records whose Status matches the given value
func (r *DownloadRepository) GetByStatus(status string) ([]*models.PendingDownload, error) {
	all, err := r.GetAll()
	if err != nil {
		return nil, err
	}

	filtered := make([]*models.PendingDownload, 0)
	for _, d := range all {
		if d.Status == status {
			filtered = append(filtered, d)
		}
	}

	return filtered, nil
}

// Delete removes a PendingDownload record by its torrent hash
func (r *DownloadRepository) Delete(hash string) error {
	return r.db.Update(func(tx *nutsdb.Tx) error {
		return tx.Delete(DownloadBucket, []byte(hash))
	})
}

// UpdateStatus loads the record for the given hash, sets Status, and re-saves it
func (r *DownloadRepository) UpdateStatus(hash, status string) error {
	d, err := r.GetByHash(hash)
	if err != nil {
		return err
	}

	d.Status = status
	d.UpdatedAt = time.Now()

	return r.Save(d)
}
