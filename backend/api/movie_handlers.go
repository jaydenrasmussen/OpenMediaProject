package api

import (
	"fmt"
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

	return c.JSON(MovieResponse{
		Movies: movies,
		Count:  len(movies),
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

	cfg, _ := config.Load()

	// Enrich lazily: cast absent (new movie), or RT score not yet fetched.
	needsEnrich := len(movie.Cast) == 0 || movie.RottenTomatoes == ""
	if cfg != nil && needsEnrich {
		enrichMovie(movie, cfg)
		if err := repo.Update(movie); err != nil {
			log.Printf("GetMovie: failed to cache enriched data for %s: %v", id, err)
		}
	}

	return c.JSON(movie)
}

// enrichMovie fetches extended metadata (cast, IMDB ID, ratings) and populates
// the extra fields on movie in-place. Safe to call multiple times.
func enrichMovie(movie *models.Movie, cfg *config.Config) {
	// If we're missing the IMDB ID or cast, re-fetch full TMDB details.
	if (movie.IMDBId == "" || len(movie.Cast) == 0) && cfg.TMDB.APIKey != "" {
		tmdb := clients.NewTMDBClient(cfg.TMDB.APIKey)
		data, err := tmdb.GetMovie(movie.ID)
		if err != nil {
			log.Printf("enrichMovie: TMDB detail fetch failed for %s: %v", movie.ID, err)
		} else {
			// Extract IMDB ID
			if movie.IMDBId == "" {
				if extIDs, ok := data["external_ids"].(map[string]interface{}); ok {
					movie.IMDBId, _ = extIDs["imdb_id"].(string)
				}
			}
			// Extract content rating (US)
			if movie.ContentRating == "" {
				if relDates, ok := data["release_dates"].(map[string]interface{}); ok {
					if results, ok := relDates["results"].([]interface{}); ok {
						for _, raw := range results {
							entry, ok := raw.(map[string]interface{})
							if !ok || entry["iso_3166_1"] != "US" {
								continue
							}
							if rdList, ok := entry["release_dates"].([]interface{}); ok {
								for _, rd := range rdList {
									rdMap, ok := rd.(map[string]interface{})
									if !ok {
										continue
									}
									if cert, _ := rdMap["certification"].(string); cert != "" {
										movie.ContentRating = cert
										break
									}
								}
							}
							break
						}
					}
				}
			}
			// Extract cast if still missing
			if len(movie.Cast) == 0 {
				if credits, ok := data["credits"].(map[string]interface{}); ok {
					if castList, ok := credits["cast"].([]interface{}); ok {
						for _, raw := range castList {
							cm, ok := raw.(map[string]interface{})
							if !ok {
								continue
							}
							name, _ := cm["name"].(string)
							if name == "" {
								continue
							}
							char, _ := cm["character"].(string)
							img := ""
							if pp, ok := cm["profile_path"].(string); ok && pp != "" {
								img = "https://image.tmdb.org/t/p/w185" + pp
							}
							movie.Cast = append(movie.Cast, models.CastMember{
								Name:      name,
								Character: char,
								Image:     img,
							})
							if len(movie.Cast) >= 15 {
								break
							}
						}
					}
				}
			}
		}
	}

	// Radarr proxy: IMDb rating, Metacritic, RT (movies only, no key needed).
	if movie.IMDBId != "" && movie.RottenTomatoes == "" {
		ratings, err := clients.GetMovieRatingsFromRadarr(movie.IMDBId)
		if err != nil {
			log.Printf("enrichMovie: Radarr proxy failed for %s: %v", movie.IMDBId, err)
		} else if ratings != nil {
			movie.IMDBRating = ratings.IMDBRating
			movie.Metacritic = ratings.Metacritic
			movie.RottenTomatoes = ratings.RottenTomatoes
		}
	}

	// RT Algolia fallback: if Radarr had no RT score, search by title+year.
	if movie.RottenTomatoes == "" {
		year := 0
		if movie.ReleaseDate != "" && len(movie.ReleaseDate) >= 4 {
			fmt.Sscanf(movie.ReleaseDate[:4], "%d", &year)
		}
		rt, err := clients.GetMovieRatings(movie.Title, year)
		if err != nil {
			log.Printf("enrichMovie: RT Algolia failed for %q: %v", movie.Title, err)
		} else if rt != nil {
			movie.RottenTomatoes = fmt.Sprintf("%d%%", rt.CriticsScore)
			movie.RTAudienceScore = fmt.Sprintf("%d%%", rt.AudienceScore)
			movie.RTUrl = rt.URL
		} else {
			movie.RottenTomatoes = "N/A"
		}
	} else if movie.RTUrl == "" && movie.RottenTomatoes != "N/A" {
		// We got RT from Radarr but have no URL — build it via Algolia.
		year := 0
		if movie.ReleaseDate != "" && len(movie.ReleaseDate) >= 4 {
			fmt.Sscanf(movie.ReleaseDate[:4], "%d", &year)
		}
		if rt, err := clients.GetMovieRatings(movie.Title, year); err == nil && rt != nil {
			movie.RTAudienceScore = fmt.Sprintf("%d%%", rt.AudienceScore)
			movie.RTUrl = rt.URL
		}
	}
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
		"movies":        movies,
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
		"movies":        movies,
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

// fetchAndStoreMovies is a shared helper: calls getList, fetches full details
// for each result, upserts to DB, and writes the JSON response.
func fetchAndStoreMovies(c *fiber.Ctx, client *clients.TMDBClient, repo *db.MovieRepository, getList func() (*clients.TMDBPopularResponse, error)) error {
	listResp, err := getList()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error": true, "message": "TMDB fetch failed: " + err.Error(),
		})
	}

	movies := make([]*models.Movie, 0, len(listResp.Results))
	for _, result := range listResp.Results {
		if movieID, ok := result["id"].(float64); ok {
			id := strconv.FormatFloat(movieID, 'f', 0, 64)
			if details, err := client.GetMovie(id); err == nil {
				if m := models.NewMovieFromTMDBData(details); m.ID != "" {
					movies = append(movies, m)
				}
			} else {
				if m := models.NewMovieFromTMDBData(result); m.ID != "" {
					movies = append(movies, m)
				}
			}
		}
	}

	if err := repo.UpsertBatch(movies); err != nil {
		log.Printf("fetchAndStoreMovies: upsert failed: %v", err)
	}

	return c.JSON(fiber.Map{
		"success":     true,
		"movies":      movies,
		"count":       len(movies),
		"page":        listResp.Page,
		"total_pages": listResp.TotalPages,
	})
}

// FetchTrendingMovies fetches weekly trending movies from TMDB.
func FetchTrendingMovies(c *fiber.Ctx) error {
	cfg, err := config.Load()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error": true, "message": "Failed to load configuration: " + err.Error(),
		})
	}
	pageStr := c.Query("page", "1")
	page, err := strconv.Atoi(pageStr)
	if err != nil || page < 1 {
		page = 1
	}
	client := clients.NewTMDBClient(cfg.TMDB.APIKey)
	repo := db.NewMovieRepository()
	return fetchAndStoreMovies(c, client, repo, func() (*clients.TMDBPopularResponse, error) {
		return client.GetTrendingMovies(page)
	})
}

// FetchTopRatedMovies fetches top-rated movies from TMDB.
func FetchTopRatedMovies(c *fiber.Ctx) error {
	cfg, err := config.Load()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error": true, "message": "Failed to load configuration: " + err.Error(),
		})
	}
	pageStr := c.Query("page", "1")
	page, err := strconv.Atoi(pageStr)
	if err != nil || page < 1 {
		page = 1
	}
	client := clients.NewTMDBClient(cfg.TMDB.APIKey)
	repo := db.NewMovieRepository()
	return fetchAndStoreMovies(c, client, repo, func() (*clients.TMDBPopularResponse, error) {
		return client.GetTopRatedMovies(page)
	})
}

// FetchUpcomingMovies fetches upcoming movies from TMDB.
func FetchUpcomingMovies(c *fiber.Ctx) error {
	cfg, err := config.Load()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error": true, "message": "Failed to load configuration: " + err.Error(),
		})
	}
	pageStr := c.Query("page", "1")
	page, err := strconv.Atoi(pageStr)
	if err != nil || page < 1 {
		page = 1
	}
	client := clients.NewTMDBClient(cfg.TMDB.APIKey)
	repo := db.NewMovieRepository()
	return fetchAndStoreMovies(c, client, repo, func() (*clients.TMDBPopularResponse, error) {
		return client.GetUpcomingMovies(page)
	})
}
