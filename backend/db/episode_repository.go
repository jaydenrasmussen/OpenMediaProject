package db

import (
	"encoding/json"
	"errors"
	"fmt"
	"time"

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

// showBucket returns the per-show bucket name for the given show ID.
func showBucket(showID string) string {
	return "episodes:" + showID
}

// episodeKey returns the composite key for an episode within a per-show bucket.
func episodeKey(season, episode int) []byte {
	return []byte(fmt.Sprintf("%03d:%03d", season, episode))
}

// Save upserts an episode into the database.
func (r *EpisodeRepository) Save(ep *models.Episode) error {
	data, err := json.Marshal(ep)
	if err != nil {
		return err
	}
	return r.db.Update(func(tx *nutsdb.Tx) error {
		return tx.Put(showBucket(ep.ShowID), episodeKey(ep.Season, ep.Number), data, 0)
	})
}

// GetByKey retrieves a specific episode by show ID, season, and episode number.
func (r *EpisodeRepository) GetByKey(showID string, season, episode int) (*models.Episode, error) {
	var ep models.Episode
	err := r.db.View(func(tx *nutsdb.Tx) error {
		entry, err := tx.Get(showBucket(showID), episodeKey(season, episode))
		if err != nil {
			if err == nutsdb.ErrKeyNotFound || err == nutsdb.ErrBucketNotFound {
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

// GetByShow returns all episodes for a given show.
func (r *EpisodeRepository) GetByShow(showID string) ([]*models.Episode, error) {
	episodes := make([]*models.Episode, 0)
	err := r.db.View(func(tx *nutsdb.Tx) error {
		entries, err := tx.GetAll(showBucket(showID))
		if err != nil {
			if err == nutsdb.ErrBucketNotFound || err == nutsdb.ErrBucketEmpty {
				return nil
			}
			return err
		}
		for _, entry := range entries {
			var ep models.Episode
			if err := json.Unmarshal(entry.Value, &ep); err != nil {
				log.Warn().Err(err).Str("key", string(entry.Key)).Msg("Failed to unmarshal episode, skipping")
				continue
			}
			episodes = append(episodes, &ep)
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	return episodes, nil
}

// GetBySeason returns all episodes for a given show and season.
func (r *EpisodeRepository) GetBySeason(showID string, season int) ([]*models.Episode, error) {
	episodes := make([]*models.Episode, 0)
	err := r.db.View(func(tx *nutsdb.Tx) error {
		entries, err := tx.GetAll(showBucket(showID))
		if err != nil {
			if err == nutsdb.ErrBucketNotFound || err == nutsdb.ErrBucketEmpty {
				return nil
			}
			return err
		}
		for _, entry := range entries {
			var ep models.Episode
			if err := json.Unmarshal(entry.Value, &ep); err != nil {
				log.Warn().Err(err).Str("key", string(entry.Key)).Msg("Failed to unmarshal episode, skipping")
				continue
			}
			if ep.Season == season {
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

// GetByStatus returns all episodes with the given status across all shows.
func (r *EpisodeRepository) GetByStatus(status string) ([]*models.Episode, error) {
	showRepo := NewTVShowRepository()
	shows, err := showRepo.GetAll()
	if err != nil {
		return nil, err
	}
	var episodes []*models.Episode
	for _, show := range shows {
		showEps, err := r.GetByShow(show.ID)
		if err != nil {
			return nil, err
		}
		for _, ep := range showEps {
			if ep.Status == status {
				episodes = append(episodes, ep)
			}
		}
	}
	return episodes, nil
}

// SaveBatch upserts multiple episodes in a single transaction.
// Episodes may belong to different shows; each is written to its per-show bucket.
func (r *EpisodeRepository) SaveBatch(episodes []*models.Episode) error {
	return r.db.Update(func(tx *nutsdb.Tx) error {
		for _, ep := range episodes {
			data, err := json.Marshal(ep)
			if err != nil {
				return err
			}
			if err := tx.Put(showBucket(ep.ShowID), episodeKey(ep.Season, ep.Number), data, 0); err != nil {
				return err
			}
		}
		return nil
	})
}

// Delete removes a specific episode from the database.
func (r *EpisodeRepository) Delete(showID string, season, episode int) error {
	return r.db.Update(func(tx *nutsdb.Tx) error {
		return tx.Delete(showBucket(showID), episodeKey(season, episode))
	})
}

// DeleteByShow removes all episodes for a given show.
func (r *EpisodeRepository) DeleteByShow(showID string) error {
	bucket := showBucket(showID)
	var keys [][]byte
	err := r.db.View(func(tx *nutsdb.Tx) error {
		entries, err := tx.GetAll(bucket)
		if err != nil {
			if err == nutsdb.ErrBucketNotFound || err == nutsdb.ErrBucketEmpty {
				return nil
			}
			return err
		}
		for _, entry := range entries {
			k := make([]byte, len(entry.Key))
			copy(k, entry.Key)
			keys = append(keys, k)
		}
		return nil
	})
	if err != nil {
		return err
	}
	if len(keys) == 0 {
		return nil
	}
	return r.db.Update(func(tx *nutsdb.Tx) error {
		for _, k := range keys {
			if err := tx.Delete(bucket, k); err != nil {
				return err
			}
		}
		return nil
	})
}

// MigrateEpisodeBuckets copies episodes from the legacy flat "episodes" bucket
// into per-show "episodes:{showID}" buckets. It is idempotent: subsequent calls
// are no-ops once the migration sentinel has been written.
func MigrateEpisodeBuckets() error {
	database := GetDB()

	// Check if already migrated.
	var alreadyMigrated bool
	_ = database.View(func(tx *nutsdb.Tx) error {
		_, err := tx.Get(MigrationBucket, []byte("episodes_per_show_v1"))
		if err == nil {
			alreadyMigrated = true
		}
		return nil
	})
	if alreadyMigrated {
		log.Debug().Msg("Episode bucket migration already applied, skipping")
		return nil
	}

	// Read all episodes from the legacy flat bucket.
	var episodes []*models.Episode
	err := database.View(func(tx *nutsdb.Tx) error {
		entries, err := tx.GetAll(EpisodeBucket)
		if err != nil {
			if err == nutsdb.ErrBucketNotFound || err == nutsdb.ErrBucketEmpty {
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
				log.Warn().Err(err).Str("key", string(entry.Key)).Msg("Migration: failed to unmarshal episode, skipping")
				continue
			}
			episodes = append(episodes, &ep)
		}
		return nil
	})
	if err != nil {
		return err
	}

	log.Info().Int("count", len(episodes)).Msg("Migrating episodes to per-show buckets")

	// Write all episodes to per-show buckets.
	if len(episodes) > 0 {
		if err := database.Update(func(tx *nutsdb.Tx) error {
			for _, ep := range episodes {
				data, err := json.Marshal(ep)
				if err != nil {
					return err
				}
				if err := tx.Put(showBucket(ep.ShowID), episodeKey(ep.Season, ep.Number), data, 0); err != nil {
					return err
				}
			}
			return nil
		}); err != nil {
			return err
		}
	}

	// Mark migration as done.
	return database.Update(func(tx *nutsdb.Tx) error {
		return tx.Put(MigrationBucket, []byte("episodes_per_show_v1"), []byte(time.Now().String()), 0)
	})
}
