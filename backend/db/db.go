package db

import (
	"os"
	"sync"
	"time"

	"github.com/rs/zerolog/log"
	"github.com/xujiajun/nutsdb"
)

// DatabaseConfig holds all database related configuration
type DatabaseConfig struct {
	DBPath string // Path to NutsDB data directory
}

const (
	// Default bucket names
	UserBucket     = "users"
	TVShowBucket   = "tvshows"
	MovieBucket    = "movies"
	PlexBucket     = "plex"
	MoverBucket    = "mover"
	IndexerBucket  = "indexers"
	DownloadBucket       = "pending_downloads"
	EpisodeBucket        = "episodes"
	WantedBucket         = "wanted"
	ReleaseProfileBucket = "release_profiles"
	HistoryBucket        = "history"
	ConfigBucket         = "config"
)

var (
	instance *nutsdb.DB
	once     sync.Once
	mu       sync.Mutex
)

// Connect establishes a connection to the NutsDB database
func Connect(cfg DatabaseConfig) error {
	var err error

	once.Do(func() {
		// Ensure directory exists
		if err = os.MkdirAll(cfg.DBPath, 0755); err != nil {
			log.Error().Err(err).Str("path", cfg.DBPath).Msg("Failed to create database directory")
			return
		}
		log.Debug().Str("path", cfg.DBPath).Msg("Database directory ensured")

		// Set NutsDB options
		opt := nutsdb.DefaultOptions
		opt.Dir = cfg.DBPath
		opt.SegmentSize = 1024 * 1024 * 16 // 16MB
		opt.NodeNum = 1

		// Open database
		instance, err = nutsdb.Open(opt)
		if err != nil {
			log.Error().Err(err).Str("path", cfg.DBPath).Msg("Failed to open database")
			return
		}
		log.Debug().Str("path", cfg.DBPath).Msg("Database opened successfully")

		// Initialize buckets
		initErr := initBuckets()
		if initErr != nil {
			log.Error().Err(initErr).Msg("Failed to initialize database buckets")
		} else {
			log.Debug().Msg("Database buckets initialized")
		}
	})

	return err
}

// initBuckets creates the necessary buckets if they don't exist
func initBuckets() error {
	buckets := []string{UserBucket, TVShowBucket, MovieBucket, PlexBucket, MoverBucket, IndexerBucket, DownloadBucket, EpisodeBucket, WantedBucket, ReleaseProfileBucket, HistoryBucket, ConfigBucket}

	for _, bucket := range buckets {
		err := instance.Update(func(tx *nutsdb.Tx) error {
			// Set initialization marker with timestamp
			timestamp := []byte(time.Now().String())
			if err := tx.Put(bucket, []byte("_init"), timestamp, 0); err != nil {
				log.Error().Err(err).Str("bucket", bucket).Msg("Failed to put init marker in bucket")
				return err
			}
			log.Debug().Str("bucket", bucket).Msg("Bucket initialized")
			return nil
		})

		if err != nil {
			log.Error().Err(err).Str("bucket", bucket).Msg("Failed to initialize bucket")
			return err
		}
	}

	return nil
}

// GetDB returns the database connection instance
func GetDB() *nutsdb.DB {
	mu.Lock()
	defer mu.Unlock()
	return instance
}

// Disconnect closes the database connection
func Disconnect() error {
	if instance == nil {
		log.Debug().Msg("No database instance to disconnect")
		return nil
	}

	log.Info().Msg("Closing database connection")
	err := instance.Close()
	if err != nil {
		log.Error().Err(err).Msg("Error closing database")
	}
	return err
}
