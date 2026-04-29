package models

import (
	"encoding/json"
	"fmt"
	"strconv"
	"time"
)

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
	Genres           []string  `json:"genres"`
	FetchedAt        time.Time `json:"fetched_at"`
	PreferredQuality string    `json:"preferred_quality,omitempty"` // item-level override; "" = use global config
	Monitored        bool      `json:"monitored"`                   // true = automatically queue future episodes
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
	}

	if posterURL, ok := tvdbData["poster"].(string); ok {
		show.PosterURL = posterURL
	} else if image, ok := tvdbData["image"].(string); ok {
		show.PosterURL = image
	}

	if backdropURL, ok := tvdbData["backdrop"].(string); ok {
		show.BackdropURL = backdropURL
	}

	// Handle rating which might be a string or a number
	if rating, ok := tvdbData["rating"].(float64); ok {
		show.Rating = rating
	} else if ratingStr, ok := tvdbData["rating"].(string); ok {
		if parsed, err := strconv.ParseFloat(ratingStr, 64); err == nil {
			show.Rating = parsed
		}
	}

	// Handle genres which could be in different formats
	if genresRaw, ok := tvdbData["genres"].([]interface{}); ok {
		for _, g := range genresRaw {
			if genreStr, ok := g.(string); ok {
				show.Genres = append(show.Genres, genreStr)
			}
		}
	}

	return show
}
