package models

import (
	"encoding/json"
	"fmt"
	"strconv"
	"time"
)

// CastMember represents an actor in a TV show.
type CastMember struct {
	Name      string `json:"name"`            // actor's real name
	Character string `json:"character"`       // character name
	Image     string `json:"image,omitempty"` // profile photo URL
}

// TVShow represents a TV show from TVDB
type TVShow struct {
	ID          string    `json:"id"`
	Name        string    `json:"name"`
	Overview    string    `json:"overview"`
	FirstAired  string    `json:"first_aired"`
	Network     string    `json:"network"`
	Status      string    `json:"status"`
	PosterURL   string    `json:"poster_url"`
	BackdropURL string    `json:"backdrop_url"`
	Rating      float64   `json:"rating"`
	Genres      []string  `json:"genres"`
	FetchedAt   time.Time `json:"fetched_at"`
	// Download preferences
	PreferredQuality string `json:"preferred_quality,omitempty"`
	Monitored        bool   `json:"monitored"`
	// Enriched metadata (populated on first detail-page view)
	Cast           []CastMember `json:"cast,omitempty"`
	ContentRating  string       `json:"content_rating,omitempty"`
	IMDBId         string       `json:"imdb_id,omitempty"`
	TMDBId         string       `json:"tmdb_id,omitempty"`
	IMDBRating     string       `json:"imdb_rating,omitempty"`
	RottenTomatoes string       `json:"rotten_tomatoes,omitempty"` // critics score e.g. "89%"
	RTAudienceScore string      `json:"rt_audience_score,omitempty"` // audience score e.g. "85%"
	RTUrl          string       `json:"rt_url,omitempty"`
	Metacritic     string       `json:"metacritic,omitempty"`
	TMDBRating     float64      `json:"tmdb_rating,omitempty"`
	Popularity     float64      `json:"popularity,omitempty"`
}

// ToJSON serializes the TV show to JSON
func (t *TVShow) ToJSON() ([]byte, error) {
	return json.Marshal(t)
}

// FromJSON deserializes the TV show from JSON
func (t *TVShow) FromJSON(data []byte) error {
	return json.Unmarshal(data, t)
}

// GetKey returns the key used to store this TV show in NutsDB
func (t *TVShow) GetKey() []byte {
	return []byte(t.ID)
}

// NewTVShowFromTVDBData creates a new TVShow from TVDB API data
func NewTVShowFromTVDBData(tvdbData map[string]interface{}) *TVShow {
	show := &TVShow{
		FetchedAt: time.Now(),
	}

	// Extract ID (could be in different formats from TVDB)
	if id, ok := tvdbData["id"].(string); ok {
		show.ID = id
	} else if id, ok := tvdbData["id"].(float64); ok {
		show.ID = fmt.Sprintf("%.0f", id)
	}

	// Extract other fields with type assertions and fallbacks
	if name, ok := tvdbData["name"].(string); ok {
		show.Name = name
	}

	if overview, ok := tvdbData["overview"].(string); ok {
		show.Overview = overview
	}

	if firstAired, ok := tvdbData["firstAired"].(string); ok {
		show.FirstAired = firstAired
	}

	if network, ok := tvdbData["network"].(string); ok {
		show.Network = network
	}

	if status, ok := tvdbData["status"].(string); ok {
		show.Status = status
	} else if statusObj, ok := tvdbData["status"].(map[string]interface{}); ok {
		if name, ok := statusObj["name"].(string); ok {
			show.Status = name
		}
	}

	if posterURL, ok := tvdbData["poster"].(string); ok {
		show.PosterURL = posterURL
	} else if image, ok := tvdbData["image"].(string); ok {
		show.PosterURL = image
	}

	if backdropURL, ok := tvdbData["backdrop"].(string); ok {
		show.BackdropURL = backdropURL
	}

	// Handle rating which might be a string or a number; also check scoreAverage
	if rating, ok := tvdbData["rating"].(float64); ok {
		show.Rating = rating
	} else if ratingStr, ok := tvdbData["rating"].(string); ok {
		if parsed, err := strconv.ParseFloat(ratingStr, 64); err == nil {
			show.Rating = parsed
		}
	}
	if show.Rating == 0 {
		// scoreAverage is the 0-10 community average; "score" is a vote count so skip it.
		if avg, ok := tvdbData["scoreAverage"].(float64); ok && avg > 0 {
			show.Rating = avg
		}
	}

	// Handle genres which could be strings or objects with a "name" field
	if genresRaw, ok := tvdbData["genres"].([]interface{}); ok {
		for _, g := range genresRaw {
			switch v := g.(type) {
			case string:
				show.Genres = append(show.Genres, v)
			case map[string]interface{}:
				if name, ok := v["name"].(string); ok {
					show.Genres = append(show.Genres, name)
				}
			}
		}
	}

	return show
}

// NewTVShowFromTMDBTVData creates a TVShow from a TMDB /tv/{id} response that
// includes external_ids. The TVDB ID (from external_ids.tvdb_id) is used as
// the primary key so the record integrates with the rest of the TVDB-centric
// pipeline. Returns nil when no TVDB ID is present.
func NewTVShowFromTMDBTVData(data map[string]interface{}) *TVShow {
	extIDs, _ := data["external_ids"].(map[string]interface{})
	if extIDs == nil {
		return nil
	}
	tvdbIDFloat, ok := extIDs["tvdb_id"].(float64)
	if !ok || tvdbIDFloat == 0 {
		return nil
	}
	tvdbID := fmt.Sprintf("%.0f", tvdbIDFloat)

	show := &TVShow{
		ID:        tvdbID,
		FetchedAt: time.Now(),
	}

	if tmdbID, ok := data["id"].(float64); ok {
		show.TMDBId = fmt.Sprintf("%.0f", tmdbID)
	}
	if name, ok := data["name"].(string); ok {
		show.Name = name
	}
	if overview, ok := data["overview"].(string); ok {
		show.Overview = overview
	}
	if fa, ok := data["first_air_date"].(string); ok {
		show.FirstAired = fa
	}
	if pp, ok := data["poster_path"].(string); ok && pp != "" {
		show.PosterURL = "https://image.tmdb.org/t/p/w500" + pp
	}
	if bp, ok := data["backdrop_path"].(string); ok && bp != "" {
		show.BackdropURL = "https://image.tmdb.org/t/p/original" + bp
	}
	if rating, ok := data["vote_average"].(float64); ok {
		show.TMDBRating = rating
		show.Rating = rating
	}
	if pop, ok := data["popularity"].(float64); ok {
		show.Popularity = pop
	}
	if status, ok := data["status"].(string); ok {
		show.Status = status
	}
	if networks, ok := data["networks"].([]interface{}); ok && len(networks) > 0 {
		if net, ok := networks[0].(map[string]interface{}); ok {
			if netName, ok := net["name"].(string); ok {
				show.Network = netName
			}
		}
	}
	if genresRaw, ok := data["genres"].([]interface{}); ok {
		for _, g := range genresRaw {
			if gMap, ok := g.(map[string]interface{}); ok {
				if gName, ok := gMap["name"].(string); ok {
					show.Genres = append(show.Genres, gName)
				}
			}
		}
	}
	return show
}
