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

// TVShowResponse represents a response containing TV show data
type TVShowResponse struct {
	Shows []*models.TVShow `json:"tvshows"`
	Count int              `json:"count"`
}

// GetTVShows returns all TV shows in the database
func GetTVShows(c *fiber.Ctx) error {
	repo := db.NewTVShowRepository()
	shows, err := repo.GetAll()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error retrieving TV shows: " + err.Error(),
		})
	}

	count, _ := repo.Count()

	return c.JSON(TVShowResponse{
		Shows: shows,
		Count: count,
	})
}

// GetTVShow returns a specific TV show by ID.
// On the first detail-page view (when Cast is empty), it calls TVDB extended,
// TMDB, and optionally OMDB to enrich the record and caches the result in DB.
func GetTVShow(c *fiber.Ctx) error {
	id := c.Params("id")
	repo := db.NewTVShowRepository()

	show, err := repo.GetByID(id)
	if err != nil {
		if err == db.ErrTVShowNotFound {
			return c.Status(fiber.StatusNotFound).JSON(fiber.Map{
				"error":   true,
				"message": "TV show not found",
			})
		}
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error retrieving TV show: " + err.Error(),
		})
	}

	cfg, _ := config.Load()

	// Enrich if cast is absent, RT score is missing, or OMDB data is now available.
	needsEnrich := len(show.Cast) == 0 || show.RottenTomatoes == ""
	needsOMDB := cfg != nil && cfg.OMDB.APIKey != "" && show.IMDBId != "" && show.IMDBRating == ""
	if cfg != nil && (needsEnrich || needsOMDB) {
		enrichTVShow(show, cfg)
		if err := repo.Update(show); err != nil {
			log.Printf("GetTVShow: failed to cache enriched data for %s: %v", id, err)
		}
	}

	return c.JSON(show)
}

// enrichTVShow fetches extended metadata from TVDB, TMDB, and OMDB and
// populates the extra fields on show in-place. It never overwrites existing
// non-empty cast data so it is safe to call multiple times.
func enrichTVShow(show *models.TVShow, cfg *config.Config) {
	tvdb := clients.NewTVDBClient(cfg.TVDB.APIKey)
	ext, err := tvdb.GetShowExtended(show.ID)
	if err != nil {
		log.Printf("enrichTVShow: TVDB extended failed for %s: %v", show.ID, err)
	} else {
		applyTVDBExtended(show, ext)
	}

	// TMDB: rate, popularity, and potentially better cast
	if show.TMDBId != "" && cfg.TMDB.APIKey != "" {
		tmdb := clients.NewTMDBClient(cfg.TMDB.APIKey)
		tvData, err := tmdb.GetTVShow(show.TMDBId)
		if err != nil {
			log.Printf("enrichTVShow: TMDB TV failed for tmdb=%s: %v", show.TMDBId, err)
		} else {
			applyTMDBTV(show, tvData)
		}
	}

	// OMDB: IMDB rating, Rotten Tomatoes (fallback), Metacritic
	if show.IMDBId != "" && cfg.OMDB.APIKey != "" && show.IMDBRating == "" {
		omdb := clients.NewOMDBClient(cfg.OMDB.APIKey)
		ratings, err := omdb.GetRatings(show.IMDBId)
		if err != nil {
			log.Printf("enrichTVShow: OMDB failed for %s: %v", show.IMDBId, err)
		} else if ratings != nil {
			show.IMDBRating = ratings.IMDBRating
			// Only use OMDB RT if we didn't already get it from Algolia.
			if show.RottenTomatoes == "" {
				show.RottenTomatoes = ratings.RottenTomatoes
			}
			show.Metacritic = ratings.Metacritic
		}
	}

	// RT Algolia: critics and audience scores (no API key required).
	if show.RottenTomatoes == "" {
		year := 0
		if show.FirstAired != "" && len(show.FirstAired) >= 4 {
			fmt.Sscanf(show.FirstAired[:4], "%d", &year)
		}
		rt, err := clients.GetTVRatings(show.Name, year)
		if err != nil {
			log.Printf("enrichTVShow: RT Algolia failed for %q: %v", show.Name, err)
		} else if rt != nil {
			show.RottenTomatoes = fmt.Sprintf("%d%%", rt.CriticsScore)
			show.RTAudienceScore = fmt.Sprintf("%d%%", rt.AudienceScore)
			show.RTUrl = rt.URL
		} else {
			// No match found — use sentinel so we don't re-search every time.
			show.RottenTomatoes = "N/A"
		}
	}
}

// applyTVDBExtended parses the TVDB v4 extended series record into show fields.
func applyTVDBExtended(show *models.TVShow, ext map[string]interface{}) {
	// Cast — characters with type == 1 are actors.
	if chars, ok := ext["characters"].([]interface{}); ok && len(show.Cast) == 0 {
		var cast []models.CastMember
		for _, raw := range chars {
			ch, ok := raw.(map[string]interface{})
			if !ok {
				continue
			}
			// type 1 = actor
			if t, ok := ch["type"].(float64); !ok || t != 1 {
				continue
			}
			actor := ch["personName"]
			char := ch["name"]
			img, _ := ch["personImgURL"].(string)
			actorName, _ := actor.(string)
			charName, _ := char.(string)
			if actorName == "" {
				continue
			}
			cast = append(cast, models.CastMember{
				Name:      actorName,
				Character: charName,
				Image:     img,
			})
			if len(cast) >= 15 { // cap at 15 featured actors
				break
			}
		}
		show.Cast = cast
	}

	// Content rating — prefer USA, fall back to first available.
	if ratings, ok := ext["contentRatings"].([]interface{}); ok && show.ContentRating == "" {
		for _, raw := range ratings {
			r, ok := raw.(map[string]interface{})
			if !ok {
				continue
			}
			name, _ := r["name"].(string)
			country, _ := r["country"].(string)
			if name == "" {
				continue
			}
			if show.ContentRating == "" {
				show.ContentRating = name // take first as fallback
			}
			if country == "usa" {
				show.ContentRating = name
				break
			}
		}
	}

	// Remote IDs — extract IMDB and TMDB IDs.
	if remotes, ok := ext["remoteIds"].([]interface{}); ok {
		for _, raw := range remotes {
			r, ok := raw.(map[string]interface{})
			if !ok {
				continue
			}
			src, _ := r["sourceName"].(string)
			rid, _ := r["id"].(string)
			if rid == "" {
				continue
			}
			switch src {
			case "IMDB":
				if show.IMDBId == "" {
					show.IMDBId = rid
				}
			case "TheMovieDB.com":
				if show.TMDBId == "" {
					show.TMDBId = rid
				}
			}
		}
	}

	// Score (community average rating, 0-10). "score" is a vote count; use "scoreAverage".
	if show.Rating == 0 {
		if avg, ok := ext["scoreAverage"].(float64); ok && avg > 0 {
			show.Rating = avg
		}
	}

	// Status (v4 extended returns status as an object).
	if show.Status == "" {
		if statusObj, ok := ext["status"].(map[string]interface{}); ok {
			if name, ok := statusObj["name"].(string); ok {
				show.Status = name
			}
		}
	}
}

// applyTMDBTV parses a TMDB /tv/:id?append_to_response=credits,external_ids
// response into show fields.
func applyTMDBTV(show *models.TVShow, data map[string]interface{}) {
	if avg, ok := data["vote_average"].(float64); ok && avg > 0 {
		show.TMDBRating = avg
	}
	if pop, ok := data["popularity"].(float64); ok && pop > 0 {
		show.Popularity = pop
	}

	// Cast from TMDB credits (better images than TVDB).
	if len(show.Cast) == 0 {
		if credits, ok := data["credits"].(map[string]interface{}); ok {
			if castRaw, ok := credits["cast"].([]interface{}); ok {
				var cast []models.CastMember
				for _, raw := range castRaw {
					m, ok := raw.(map[string]interface{})
					if !ok {
						continue
					}
					name, _ := m["name"].(string)
					char, _ := m["character"].(string)
					profilePath, _ := m["profile_path"].(string)
					if name == "" {
						continue
					}
					img := ""
					if profilePath != "" {
						img = "https://image.tmdb.org/t/p/w185" + profilePath
					}
					cast = append(cast, models.CastMember{Name: name, Character: char, Image: img})
					if len(cast) >= 15 {
						break
					}
				}
				show.Cast = cast
			}
		}
	}

	// IMDB ID from TMDB external_ids if not already set.
	if show.IMDBId == "" {
		if extIDs, ok := data["external_ids"].(map[string]interface{}); ok {
			if imdb, ok := extIDs["imdb_id"].(string); ok {
				show.IMDBId = imdb
			}
		}
	}
}

// FetchPopularTVShows fetches popular TV shows from TVDB API and stores them in the database
func FetchPopularTVShows(c *fiber.Ctx) error {
	// Get configuration
	cfg, err := config.Load()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to load configuration: " + err.Error(),
		})
	}

	// Create TVDB client
	client := clients.NewTVDBClient(cfg.TVDB.APIKey)

	// Fetch popular shows
	rawShows, err := client.GetPopularShows()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to fetch popular TV shows: " + err.Error(),
		})
	}

	// Convert to our model
	shows := make([]*models.TVShow, 0, len(rawShows))
	for _, rawShow := range rawShows {
		show := models.NewTVShowFromTVDBData(rawShow)
		if show.ID != "" { // Only add shows with valid IDs
			shows = append(shows, show)
		}
	}

	// Save to database
	repo := db.NewTVShowRepository()
	if err := repo.UpsertBatch(shows); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to save TV shows: " + err.Error(),
		})
	}

	return c.JSON(fiber.Map{
		"success": true,
		"message": "Successfully fetched and stored popular TV shows",
		"shows":   shows,
		"count":   len(shows),
	})
}

// SearchTVShows searches for TV shows by name.
// It checks the local DB first; only falls back to TVDB if no local results are found.
func SearchTVShows(c *fiber.Ctx) error {
	query := c.Query("q")
	if query == "" {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Search query is required",
		})
	}

	// Try local DB first
	repo := db.NewTVShowRepository()
	localResults, err := repo.Search(query)
	if err != nil {
		log.Printf("Error searching local TV shows: %v", err)
		// Continue to API search even if local search fails
	} else if len(localResults) > 0 {
		return c.JSON(TVShowResponse{
			Shows: localResults,
			Count: len(localResults),
		})
	}

	// No local results — try TVDB API
	cfg, err := config.Load()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to load configuration: " + err.Error(),
		})
	}

	// Create TVDB client and search
	client := clients.NewTVDBClient(cfg.TVDB.APIKey)
	rawShows, err := client.SearchShows(query)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to search TV shows: " + err.Error(),
		})
	}

	// Convert to our model
	shows := make([]*models.TVShow, 0, len(rawShows))
	for _, rawShow := range rawShows {
		show := models.NewTVShowFromTVDBData(rawShow)
		if show.ID != "" {
			shows = append(shows, show)
		}
	}

	// Save to database
	if err := repo.UpsertBatch(shows); err != nil {
		log.Printf("Failed to save searched TV shows: %v", err)
		// We continue anyway to return search results
	}

	return c.JSON(TVShowResponse{
		Shows: shows,
		Count: len(shows),
	})
}

// GetRecentTVShows returns TV shows that were recently updated
func GetRecentTVShows(c *fiber.Ctx) error {
	// Parse days parameter (default to 7 days)
	daysStr := c.Query("days", "7")
	days, err := strconv.Atoi(daysStr)
	if err != nil || days <= 0 {
		days = 7
	}

	// Calculate cutoff time
	cutoff := time.Now().Add(-time.Duration(days) * 24 * time.Hour)

	// Get recent shows
	repo := db.NewTVShowRepository()
	shows, err := repo.GetRecentlyUpdated(cutoff)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error retrieving recent TV shows: " + err.Error(),
		})
	}

	return c.JSON(TVShowResponse{
		Shows: shows,
		Count: len(shows),
	})
}

// DeleteTVShow deletes a TV show by ID
func DeleteTVShow(c *fiber.Ctx) error {
	id := c.Params("id")
	repo := db.NewTVShowRepository()

	// Check if TV show exists
	_, err := repo.GetByID(id)
	if err != nil {
		if err == db.ErrTVShowNotFound {
			return c.Status(fiber.StatusNotFound).JSON(fiber.Map{
				"error":   true,
				"message": "TV show not found",
			})
		}

		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error retrieving TV show: " + err.Error(),
		})
	}

	// Delete the TV show
	if err := repo.Delete(id); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Error deleting TV show: " + err.Error(),
		})
	}

	return c.SendStatus(fiber.StatusNoContent)
}

// SetTVShowQuality handles PATCH /api/v1/tvshows/:id/quality.
// It sets (or clears) the per-item preferred quality override.
func SetTVShowQuality(c *fiber.Ctx) error {
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

	repo := db.NewTVShowRepository()
	show, err := repo.GetByID(id)
	if err != nil {
		if err == db.ErrTVShowNotFound {
			return c.Status(fiber.StatusNotFound).JSON(fiber.Map{
				"error":   true,
				"message": "TV show not found",
			})
		}
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to retrieve TV show: " + err.Error(),
		})
	}

	show.PreferredQuality = body.Quality

	if err := repo.Update(show); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to update TV show: " + err.Error(),
		})
	}

	return c.JSON(fiber.Map{
		"success": true,
		"quality": show.PreferredQuality,
	})
}

// FetchTrendingTV fetches weekly trending TV shows from TMDB, maps each result
// to its TVDB ID via external_ids, and upserts into the DB so they appear on
// the discover page using the same TVDB-based routing as all other TV shows.
func FetchTrendingTV(c *fiber.Ctx) error {
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

	tmdb := clients.NewTMDBClient(cfg.TMDB.APIKey)
	trendingResp, err := tmdb.GetTrendingTV(page)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error": true, "message": "TMDB trending TV fetch failed: " + err.Error(),
		})
	}

	repo := db.NewTVShowRepository()
	shows := make([]*models.TVShow, 0, len(trendingResp.Results))

	for _, result := range trendingResp.Results {
		tmdbID, ok := result["id"].(float64)
		if !ok {
			continue
		}
		tmdbIDStr := strconv.FormatFloat(tmdbID, 'f', 0, 64)

		// GetTVShow already appends external_ids so one call gives us the TVDB ID.
		tvData, err := tmdb.GetTVShow(tmdbIDStr)
		if err != nil {
			log.Printf("FetchTrendingTV: GetTVShow(%s) failed: %v", tmdbIDStr, err)
			continue
		}

		show := models.NewTVShowFromTMDBTVData(tvData)
		if show != nil {
			shows = append(shows, show)
		}
	}

	if err := repo.UpsertBatch(shows); err != nil {
		log.Printf("FetchTrendingTV: upsert failed: %v", err)
	}

	return c.JSON(fiber.Map{
		"success":     true,
		"shows":       shows,
		"count":       len(shows),
		"page":        trendingResp.Page,
		"total_pages": trendingResp.TotalPages,
	})
}
