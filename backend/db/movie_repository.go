package db

import (
	"encoding/json"
	"errors"
	"strings"
	"time"

	"backend/models"

	"github.com/xujiajun/nutsdb"
)

var (
	ErrMovieNotFound = errors.New("movie not found")
	ErrMovieExists   = errors.New("movie with this ID already exists")
)

// MovieRepository handles movie data operations with NutsDB
type MovieRepository struct {
	db *nutsdb.DB
}

// NewMovieRepository creates a new movie repository
func NewMovieRepository() *MovieRepository {
	return &MovieRepository{
		db: GetDB(),
	}
}

// Create adds a new movie to the database
func (r *MovieRepository) Create(movie *models.Movie) error {
	// Convert movie to JSON
	movieData, err := json.Marshal(movie)
	if err != nil {
		return err
	}

	// Store in database
	return r.db.Update(func(tx *nutsdb.Tx) error {
		// Check if key already exists
		_, err := tx.Get(MovieBucket, []byte(movie.ID))
		if err == nil {
			return ErrMovieExists
		}
		if err != nutsdb.ErrKeyNotFound {
			return err
		}

		// Store movie data
		return tx.Put(MovieBucket, []byte(movie.ID), movieData, 0) // 0 means no expiration
	})
}

// Update updates an existing movie
func (r *MovieRepository) Update(movie *models.Movie) error {
	// Convert to JSON
	movieData, err := json.Marshal(movie)
	if err != nil {
		return err
	}

	return r.db.Update(func(tx *nutsdb.Tx) error {
		// Check if movie exists
		_, err := tx.Get(MovieBucket, []byte(movie.ID))
		if err != nil {
			if err == nutsdb.ErrKeyNotFound {
				return ErrMovieNotFound
			}
			return err
		}

		// Update movie
		return tx.Put(MovieBucket, []byte(movie.ID), movieData, 0)
	})
}

// GetByID retrieves a movie by ID
func (r *MovieRepository) GetByID(id string) (*models.Movie, error) {
	var movie models.Movie

	err := r.db.View(func(tx *nutsdb.Tx) error {
		entry, err := tx.Get(MovieBucket, []byte(id))
		if err != nil {
			if err == nutsdb.ErrKeyNotFound {
				return ErrMovieNotFound
			}
			return err
		}

		// Deserialize data
		return json.Unmarshal(entry.Value, &movie)
	})

	if err != nil {
		return nil, err
	}

	return &movie, nil
}

// GetAll retrieves all movies
func (r *MovieRepository) GetAll() ([]*models.Movie, error) {
	movies := make([]*models.Movie, 0)

	err := r.db.View(func(tx *nutsdb.Tx) error {
		entries, err := tx.GetAll(MovieBucket)
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

			var movie models.Movie
			if err := json.Unmarshal(entry.Value, &movie); err != nil {
				return err
			}
			movies = append(movies, &movie)
		}

		return nil
	})

	if err != nil {
		return nil, err
	}

	return movies, nil
}

// Delete deletes a movie by ID
func (r *MovieRepository) Delete(id string) error {
	return r.db.Update(func(tx *nutsdb.Tx) error {
		return tx.Delete(MovieBucket, []byte(id))
	})
}

// UpsertBatch upserts multiple movies in a single transaction
func (r *MovieRepository) UpsertBatch(movies []*models.Movie) error {
	return r.db.Update(func(tx *nutsdb.Tx) error {
		for _, movie := range movies {
			// Convert movie to JSON
			movieData, err := json.Marshal(movie)
			if err != nil {
				return err
			}

			// Put the movie in the database
			if err := tx.Put(MovieBucket, []byte(movie.ID), movieData, 0); err != nil {
				return err
			}
		}
		return nil
	})
}

// GetRecentlyUpdated retrieves movies updated after a certain time
func (r *MovieRepository) GetRecentlyUpdated(since time.Time) ([]*models.Movie, error) {
	movies := make([]*models.Movie, 0)

	err := r.db.View(func(tx *nutsdb.Tx) error {
		entries, err := tx.GetAll(MovieBucket)
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

			var movie models.Movie
			if err := json.Unmarshal(entry.Value, &movie); err != nil {
				return err
			}

			if movie.FetchedAt.After(since) {
				movies = append(movies, &movie)
			}
		}

		return nil
	})

	if err != nil {
		return nil, err
	}

	return movies, nil
}

// Count returns the number of movies in the database
func (r *MovieRepository) Count() (int, error) {
	var count int

	err := r.db.View(func(tx *nutsdb.Tx) error {
		entries, err := tx.GetAll(MovieBucket)
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

// Search searches for movies by title
func (r *MovieRepository) Search(query string) ([]*models.Movie, error) {
	results := make([]*models.Movie, 0)
	query = strings.ToLower(query)

	err := r.db.View(func(tx *nutsdb.Tx) error {
		entries, err := tx.GetAll(MovieBucket)
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

			var movie models.Movie
			if err := json.Unmarshal(entry.Value, &movie); err != nil {
				return err
			}

			// Check if title contains the query
			if strings.Contains(strings.ToLower(movie.Title), query) {
				results = append(results, &movie)
			}
		}

		return nil
	})

	if err != nil {
		return nil, err
	}

	return results, nil
}
