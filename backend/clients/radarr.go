package clients

// RadarrRatings holds IMDb, Metacritic, and Rotten Tomatoes scores sourced
// from the public Radarr IMDB proxy (https://api.radarr.video/v1).
//
// The proxy is operated by the Radarr project and is used by all Radarr
// instances worldwide. It caches IMDB data and exposes it at no cost.
// This endpoint works for movies only — TV show IMDB IDs return empty.

import (
	"encoding/json"
	"fmt"
	"net/http"
)

const radarrProxyBase = "https://api.radarr.video/v1"

// RadarrRatings holds aggregated rating data for a movie.
type RadarrRatings struct {
	IMDBRating     string // e.g. "8.8"
	Metacritic     string // e.g. "74"
	RottenTomatoes string // e.g. "87"
}

// GetMovieRatingsFromRadarr fetches IMDb, Metacritic, and RT scores for a
// movie identified by its IMDB ID using the Radarr public proxy.
// Returns nil (no error) when the movie is not found or has no ratings.
func GetMovieRatingsFromRadarr(imdbID string) (*RadarrRatings, error) {
	url := fmt.Sprintf("%s/movie/imdb/%s", radarrProxyBase, imdbID)
	resp, err := http.Get(url)
	if err != nil {
		return nil, fmt.Errorf("radarr proxy: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("radarr proxy: status %d", resp.StatusCode)
	}

	var results []struct {
		ImdbId       string `json:"ImdbId"`
		MovieRatings struct {
			Imdb struct {
				Value float64 `json:"Value"`
			} `json:"Imdb"`
			Metacritic struct {
				Value float64 `json:"Value"`
			} `json:"Metacritic"`
			RottenTomatoes struct {
				Value float64 `json:"Value"`
			} `json:"RottenTomatoes"`
		} `json:"MovieRatings"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&results); err != nil {
		return nil, fmt.Errorf("radarr proxy decode: %w", err)
	}
	if len(results) == 0 || results[0].ImdbId != imdbID {
		return nil, nil
	}

	r := results[0].MovieRatings
	out := &RadarrRatings{}
	if r.Imdb.Value > 0 {
		out.IMDBRating = fmt.Sprintf("%.1f", r.Imdb.Value)
	}
	if r.Metacritic.Value > 0 {
		out.Metacritic = fmt.Sprintf("%.0f", r.Metacritic.Value)
	}
	if r.RottenTomatoes.Value > 0 {
		out.RottenTomatoes = fmt.Sprintf("%.0f%%", r.RottenTomatoes.Value)
	}
	return out, nil
}
