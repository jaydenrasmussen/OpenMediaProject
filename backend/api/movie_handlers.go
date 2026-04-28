package api

import (
	"log"
	"strconv"
	"time"

	"backend/clients"
	"backend/config"
	"backend/db"
	"backend/models"

	"github.com/gofiber/fiber/v2"
)

// MovieResponse represents a response containing movie data
type MovieResponse struct {
	Movies []*models.Movie `json:"movies"`
	Count  int             `json:"count"`
}

// GetMovies returns all movies in the database
func GetMovies(c *fiber.Ctx) error {
	repo := db.NewMovieRepository()
	movies, err := repo.GetAll()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error retrieving movies: " + err.Error(),
		})
	}

	count, _ := repo.Count()

	return c.JSON(MovieResponse{
		Movies: movies,
		Count:  count,
	})
}

// GetMovie returns a specific movie by ID
func GetMovie(c *fiber.Ctx) error {
	id := c.Params("id")
	repo := db.NewMovieRepository()

	movie, err := repo.GetByID(id)
	if err != nil {
		if err == db.ErrMovieNotFound {
			return c.Status(fiber.StatusNotFound).JSON(fiber.Map{
				"error":   true,
				"message": "Movie not found",
			})
		}

		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error retrieving movie: " + err.Error(),
		})
	}

	return c.JSON(movie)
}

// FetchPopularMovies fetches popular movies from TMDB API and stores them in the database
func FetchPopularMovies(c *fiber.Ctx) error {
	// Get configuration
	cfg, err := config.Load()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to load configuration: " + err.Error(),
		})
	}

	// Check if TMDB API key is configured
	if cfg.TMDB.APIKey == "" {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "TMDB API key is not configured",
		})
	}

	// Parse page parameter (default to 1)
	pageStr := c.Query("page", "1")
	page, err := strconv.Atoi(pageStr)
	if err != nil || page < 1 {
		page = 1
	}

	// Create TMDB client
	client := clients.NewTMDBClient(cfg.TMDB.APIKey)

	// Fetch popular movies
	popularResp, err := client.GetPopularMovies(page)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to fetch popular movies: " + err.Error(),
		})
	}

	// Process results
	movies := make([]*models.Movie, 0, len(popularResp.Results))
	for _, result := range popularResp.Results {
		// Fetch full movie details
		if movieID, ok := result["id"].(float64); ok {
			id := strconv.FormatFloat(movieID, 'f', 0, 64)

			// Try to get full details
			movieDetails, err := client.GetMovie(id)
			if err == nil {
				// Use detailed data if available
				movie := models.NewMovieFromTMDBData(movieDetails)
				if movie.ID != "" {
					movies = append(movies, movie)
				}
			} else {
				// Fall back to basic data
				movie := models.NewMovieFromTMDBData(result)
				if movie.ID != "" {
					movies = append(movies, movie)
				}
			}
		}
	}

	// Save to database
	repo := db.NewMovieRepository()
	if err := repo.UpsertBatch(movies); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to save movies: " + err.Error(),
		})
	}

	return c.JSON(fiber.Map{
		"success":       true,
		"message":       "Successfully fetched and stored popular movies",
		"count":         len(movies),
		"page":          page,
		"total_pages":   popularResp.TotalPages,
		"total_results": popularResp.TotalResults,
	})
}

// SearchMovies searches for movies by title using TMDB API
func SearchMovies(c *fiber.Ctx) error {
	query := c.Query("q")
	if query == "" {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Search query is required",
		})
	}

	// Parse page parameter (default to 1)
	pageStr := c.Query("page", "1")
	page, err := strconv.Atoi(pageStr)
	if err != nil || page < 1 {
		page = 1
	}

	// Get configuration
	cfg, err := config.Load()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to load configuration: " + err.Error(),
		})
	}

	// Check if TMDB API key is configured
	if cfg.TMDB.APIKey == "" {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "TMDB API key is not configured",
		})
	}

	// Create TMDB client
	client := clients.NewTMDBClient(cfg.TMDB.APIKey)

	// First try to search locally
	repo := db.NewMovieRepository()
	localResults, err := repo.Search(query)
	if err != nil {
		log.Printf("Error searching local movies: %v", err)
		// Continue to API search even if local search fails
	} else if len(localResults) > 0 {
		// If we have local results, return them
		return c.JSON(MovieResponse{
			Movies: localResults,
			Count:  len(localResults),
		})
	}

	// Search for movies using the API
	searchResp, err := client.SearchMovies(query, page)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to search movies: " + err.Error(),
		})
	}

	// Process results
	movies := make([]*models.Movie, 0, len(searchResp.Results))
	for _, result := range searchResp.Results {
		movie := models.NewMovieFromTMDBData(result)
		if movie.ID != "" {
			movies = append(movies, movie)
		}
	}

	// Save to database
	if err := repo.UpsertBatch(movies); err != nil {
		log.Printf("Failed to save searched movies: %v", err)
		// We continue anyway to return search results
	}

	return c.JSON(MovieResponse{
		Movies: movies,
		Count:  len(movies),
	})
}

// GetRecentMovies returns movies that were recently updated
func GetRecentMovies(c *fiber.Ctx) error {
	// Parse days parameter (default to 7 days)
	daysStr := c.Query("days", "7")
	days, err := strconv.Atoi(daysStr)
	if err != nil || days <= 0 {
		days = 7
	}

	// Calculate cutoff time
	cutoff := time.Now().Add(-time.Duration(days) * 24 * time.Hour)

	// Get recent movies
	repo := db.NewMovieRepository()
	movies, err := repo.GetRecentlyUpdated(cutoff)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error retrieving recent movies: " + err.Error(),
		})
	}

	return c.JSON(MovieResponse{
		Movies: movies,
		Count:  len(movies),
	})
}

// DeleteMovie deletes a movie by ID
func DeleteMovie(c *fiber.Ctx) error {
	id := c.Params("id")
	repo := db.NewMovieRepository()

	// Check if movie exists
	_, err := repo.GetByID(id)
	if err != nil {
		if err == db.ErrMovieNotFound {
			return c.Status(fiber.StatusNotFound).JSON(fiber.Map{
				"error":   true,
				"message": "Movie not found",
			})
		}

		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error retrieving movie: " + err.Error(),
		})
	}

	// Delete the movie
	if err := repo.Delete(id); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error deleting movie: " + err.Error(),
		})
	}

	return c.SendStatus(fiber.StatusNoContent)
}

// FetchNowPlayingMovies fetches movies currently in theaters
func FetchNowPlayingMovies(c *fiber.Ctx) error {
	// Get configuration
	cfg, err := config.Load()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to load configuration: " + err.Error(),
		})
	}

	// Check if TMDB API key is configured
	if cfg.TMDB.APIKey == "" {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "TMDB API key is not configured",
		})
	}

	// Parse page parameter (default to 1)
	pageStr := c.Query("page", "1")
	page, err := strconv.Atoi(pageStr)
	if err != nil || page < 1 {
		page = 1
	}

	// Create TMDB client
	client := clients.NewTMDBClient(cfg.TMDB.APIKey)

	// Fetch now playing movies
	nowPlayingResp, err := client.GetNowPlayingMovies(page)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to fetch now playing movies: " + err.Error(),
		})
	}

	// Process results
	movies := make([]*models.Movie, 0, len(nowPlayingResp.Results))
	for _, result := range nowPlayingResp.Results {
		movie := models.NewMovieFromTMDBData(result)
		if movie.ID != "" {
			movies = append(movies, movie)
		}
	}

	// Save to database
	repo := db.NewMovieRepository()
	if err := repo.UpsertBatch(movies); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to save movies: " + err.Error(),
		})
	}

	return c.JSON(fiber.Map{
		"success":       true,
		"message":       "Successfully fetched and stored now playing movies",
		"count":         len(movies),
		"page":          page,
		"total_pages":   nowPlayingResp.TotalPages,
		"total_results": nowPlayingResp.TotalResults,
	})
}

// SetMovieQuality handles PATCH /api/v1/movies/:id/quality.
// It sets (or clears) the per-item preferred quality override.
func SetMovieQuality(c *fiber.Ctx) error {
	id := c.Params("id")

	var body struct {
		Quality string `json:"quality"`
	}
	if err := c.BodyParser(&body); err != nil {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Invalid request body: " + err.Error(),
		})
	}

	repo := db.NewMovieRepository()
	movie, err := repo.GetByID(id)
	if err != nil {
		if err == db.ErrMovieNotFound {
			return c.Status(fiber.StatusNotFound).JSON(fiber.Map{
				"error":   true,
				"message": "Movie not found",
			})
		}
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to retrieve movie: " + err.Error(),
		})
	}

	movie.PreferredQuality = body.Quality

	if err := repo.Update(movie); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to update movie: " + err.Error(),
		})
	}

	return c.JSON(fiber.Map{
		"success": true,
		"quality": movie.PreferredQuality,
	})
}
