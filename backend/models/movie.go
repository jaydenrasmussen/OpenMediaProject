package models

import (
	"encoding/json"
	"fmt"
	"strconv"
	"time"
)

// Movie represents a movie from a movie API
type Movie struct {
	ID            string    `json:"id"`
	Title         string    `json:"title"`
	Overview      string    `json:"overview"`
	ReleaseDate   string    `json:"release_date"`
	Runtime       int       `json:"runtime"`
	PosterURL     string    `json:"poster_url"`
	BackdropURL   string    `json:"backdrop_url"`
	Rating        float64   `json:"rating"`
	VoteCount     int       `json:"vote_count"`
	Genres        []string  `json:"genres"`
	Director      string    `json:"director"`
	ProductionCo  string    `json:"production_company"`
	Budget        int64     `json:"budget"`
	Revenue       int64     `json:"revenue"`
	OriginalLang  string    `json:"original_language"`
	FetchedAt     time.Time `json:"fetched_at"`
	PopularityIdx    float64   `json:"popularity_index"`
	PreferredQuality string    `json:"preferred_quality,omitempty"` // item-level override; "" = use global config
}

// ToJSON serializes the movie to JSON
func (m *Movie) ToJSON() ([]byte, error) {
	return json.Marshal(m)
}

// FromJSON deserializes the movie from JSON
func (m *Movie) FromJSON(data []byte) error {
	return json.Unmarshal(data, m)
}

// GetKey returns the key used to store this movie in NutsDB
func (m *Movie) GetKey() []byte {
	return []byte(m.ID)
}

// NewMovieFromTMDBData creates a new Movie from TMDB API data
func NewMovieFromTMDBData(tmdbData map[string]interface{}) *Movie {
	movie := &Movie{
		FetchedAt: time.Now(),
	}

	// Extract ID (could be in different formats)
	if id, ok := tmdbData["id"].(float64); ok {
		movie.ID = fmt.Sprintf("%.0f", id)
	} else if id, ok := tmdbData["id"].(string); ok {
		movie.ID = id
	}

	// Extract other fields with type assertions and fallbacks
	if title, ok := tmdbData["title"].(string); ok {
		movie.Title = title
	} else if title, ok := tmdbData["original_title"].(string); ok {
		movie.Title = title
	}

	if overview, ok := tmdbData["overview"].(string); ok {
		movie.Overview = overview
	}

	if releaseDate, ok := tmdbData["release_date"].(string); ok {
		movie.ReleaseDate = releaseDate
	}

	if runtime, ok := tmdbData["runtime"].(float64); ok {
		movie.Runtime = int(runtime)
	}

	if posterPath, ok := tmdbData["poster_path"].(string); ok {
		movie.PosterURL = "https://image.tmdb.org/t/p/w500" + posterPath
	}

	if backdropPath, ok := tmdbData["backdrop_path"].(string); ok {
		movie.BackdropURL = "https://image.tmdb.org/t/p/original" + backdropPath
	}

	// Handle vote average which might be a string or a number
	if rating, ok := tmdbData["vote_average"].(float64); ok {
		movie.Rating = rating
	} else if ratingStr, ok := tmdbData["vote_average"].(string); ok {
		if parsed, err := strconv.ParseFloat(ratingStr, 64); err == nil {
			movie.Rating = parsed
		}
	}

	// Handle vote count
	if voteCount, ok := tmdbData["vote_count"].(float64); ok {
		movie.VoteCount = int(voteCount)
	}

	// Handle budget
	if budget, ok := tmdbData["budget"].(float64); ok {
		movie.Budget = int64(budget)
	}

	// Handle revenue
	if revenue, ok := tmdbData["revenue"].(float64); ok {
		movie.Revenue = int64(revenue)
	}

	// Handle original language
	if origLang, ok := tmdbData["original_language"].(string); ok {
		movie.OriginalLang = origLang
	}

	// Handle popularity
	if popularity, ok := tmdbData["popularity"].(float64); ok {
		movie.PopularityIdx = popularity
	}

	// Handle genres which could be in different formats
	if genresRaw, ok := tmdbData["genres"].([]interface{}); ok {
		for _, g := range genresRaw {
			if genreMap, ok := g.(map[string]interface{}); ok {
				if genreName, ok := genreMap["name"].(string); ok {
					movie.Genres = append(movie.Genres, genreName)
				}
			} else if genreStr, ok := g.(string); ok {
				movie.Genres = append(movie.Genres, genreStr)
			}
		}
	}

	// Try to extract director and production company if available
	if credits, ok := tmdbData["credits"].(map[string]interface{}); ok {
		if crew, ok := credits["crew"].([]interface{}); ok {
			for _, crewMember := range crew {
				if crewMap, ok := crewMember.(map[string]interface{}); ok {
					if job, ok := crewMap["job"].(string); ok && job == "Director" {
						if name, ok := crewMap["name"].(string); ok {
							movie.Director = name
							break
						}
					}
				}
			}
		}
	}

	if productionCompanies, ok := tmdbData["production_companies"].([]interface{}); ok && len(productionCompanies) > 0 {
		if company, ok := productionCompanies[0].(map[string]interface{}); ok {
			if name, ok := company["name"].(string); ok {
				movie.ProductionCo = name
			}
		}
	}

	return movie
}
