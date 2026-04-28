package db

import (
	"encoding/json"
	"errors"
	"fmt"

	"backend/models"

	"github.com/rs/zerolog/log"
	"github.com/xujiajun/nutsdb"
)

var ErrEpisodeNotFound = errors.New("episode not found")

// EpisodeRepository handles episode data operations with NutsDB
type EpisodeRepository struct {
	db *nutsdb.DB
}

// NewEpisodeRepository creates a new episode repository
func NewEpisodeRepository() *EpisodeRepository {
	return &EpisodeRepository{
		db: GetDB(),
	}
}

// episodeKey returns the composite key for an episode
func episodeKey(showID string, season, episode int) []byte {
	return []byte(fmt.Sprintf("%s:%03d:%03d", showID, season, episode))
}

// Save upserts an episode into the database
func (r *EpisodeRepository) Save(ep *models.Episode) error {
	data, err := json.Marshal(ep)
	if err != nil {
		return err
	}

	return r.db.Update(func(tx *nutsdb.Tx) error {
		return tx.Put(EpisodeBucket, episodeKey(ep.ShowID, ep.Season, ep.Number), data, 0)
	})
}

// GetByKey retrieves a specific episode by show ID, season, and episode number
func (r *EpisodeRepository) GetByKey(showID string, season, episode int) (*models.Episode, error) {
	var ep models.Episode

	err := r.db.View(func(tx *nutsdb.Tx) error {
		entry, err := tx.Get(EpisodeBucket, episodeKey(showID, season, episode))
		if err != nil {
			if err == nutsdb.ErrKeyNotFound {
				return ErrEpisodeNotFound
			}
			return err
		}
		return json.Unmarshal(entry.Value, &ep)
	})

	if err != nil {
		return nil, err
	}

	return &ep, nil
}

// GetByShow returns all episodes for a given show
func (r *EpisodeRepository) GetByShow(showID string) ([]*models.Episode, error) {
	episodes := make([]*models.Episode, 0)

	err := r.db.View(func(tx *nutsdb.Tx) error {
		entries, err := tx.GetAll(EpisodeBucket)
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

			var ep models.Episode
			if err := json.Unmarshal(entry.Value, &ep); err != nil {
				log.Warn().Err(err).Str("key", string(entry.Key)).Msg("Failed to unmarshal episode, skipping")
				continue
			}

			if ep.ShowID == showID {
				episodes = append(episodes, &ep)
			}
		}

		return nil
	})

	if err != nil {
		return nil, err
	}

	return episodes, nil
}

// GetBySeason returns all episodes for a given show and season
func (r *EpisodeRepository) GetBySeason(showID string, season int) ([]*models.Episode, error) {
	episodes := make([]*models.Episode, 0)

	err := r.db.View(func(tx *nutsdb.Tx) error {
		entries, err := tx.GetAll(EpisodeBucket)
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

			var ep models.Episode
			if err := json.Unmarshal(entry.Value, &ep); err != nil {
				log.Warn().Err(err).Str("key", string(entry.Key)).Msg("Failed to unmarshal episode, skipping")
				continue
			}

			if ep.ShowID == showID && ep.Season == season {
				episodes = append(episodes, &ep)
			}
		}

		return nil
	})

	if err != nil {
		return nil, err
	}

	return episodes, nil
}

// GetByStatus returns all episodes with the given status
func (r *EpisodeRepository) GetByStatus(status string) ([]*models.Episode, error) {
	episodes := make([]*models.Episode, 0)

	err := r.db.View(func(tx *nutsdb.Tx) error {
		entries, err := tx.GetAll(EpisodeBucket)
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

			var ep models.Episode
			if err := json.Unmarshal(entry.Value, &ep); err != nil {
				log.Warn().Err(err).Str("key", string(entry.Key)).Msg("Failed to unmarshal episode, skipping")
				continue
			}

			if ep.Status == status {
				episodes = append(episodes, &ep)
			}
		}

		return nil
	})

	if err != nil {
		return nil, err
	}

	return episodes, nil
}

// SaveBatch upserts multiple episodes in a single transaction
func (r *EpisodeRepository) SaveBatch(episodes []*models.Episode) error {
	return r.db.Update(func(tx *nutsdb.Tx) error {
		for _, ep := range episodes {
			data, err := json.Marshal(ep)
			if err != nil {
				return err
			}

			if err := tx.Put(EpisodeBucket, episodeKey(ep.ShowID, ep.Season, ep.Number), data, 0); err != nil {
				return err
			}
		}
		return nil
	})
}

// Delete removes a specific episode from the database
func (r *EpisodeRepository) Delete(showID string, season, episode int) error {
	return r.db.Update(func(tx *nutsdb.Tx) error {
		return tx.Delete(EpisodeBucket, episodeKey(showID, season, episode))
	})
}

// DeleteByShow removes all episodes for a given show
func (r *EpisodeRepository) DeleteByShow(showID string) error {
	// Collect keys first in a read transaction, then delete in a write transaction
	var keysToDelete [][]byte

	err := r.db.View(func(tx *nutsdb.Tx) error {
		entries, err := tx.GetAll(EpisodeBucket)
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

			var ep models.Episode
			if err := json.Unmarshal(entry.Value, &ep); err != nil {
				log.Warn().Err(err).Str("key", string(entry.Key)).Msg("Failed to unmarshal episode during DeleteByShow, skipping")
				continue
			}

			if ep.ShowID == showID {
				// Copy the key bytes to avoid holding a reference into the tx buffer
				keyCopy := make([]byte, len(entry.Key))
				copy(keyCopy, entry.Key)
				keysToDelete = append(keysToDelete, keyCopy)
			}
		}

		return nil
	})

	if err != nil {
		return err
	}

	if len(keysToDelete) == 0 {
		return nil
	}

	return r.db.Update(func(tx *nutsdb.Tx) error {
		for _, key := range keysToDelete {
			if err := tx.Delete(EpisodeBucket, key); err != nil {
				return err
			}
		}
		return nil
	})
}
