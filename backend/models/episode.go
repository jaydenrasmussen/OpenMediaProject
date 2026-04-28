package models

import (
	"fmt"
	"time"
)

// Episode status constants
const (
	EpisodeStatusWanted      = "wanted"
	EpisodeStatusDownloading = "downloading"
	EpisodeStatusDownloaded  = "downloaded"
	EpisodeStatusSkipped     = "skipped"
	EpisodeStatusMissing     = "missing"
)

// Episode represents a single TV episode
type Episode struct {
	ID           string     `json:"id"`      // TVDB episode ID (string)
	ShowID       string     `json:"show_id"` // parent TVShow.ID
	Season       int        `json:"season"`
	Number       int        `json:"number"` // episode number within season
	Title        string     `json:"title"`
	AirDate      string     `json:"air_date"` // "YYYY-MM-DD"
	Overview     string     `json:"overview"`
	Status       string     `json:"status"` // see EpisodeStatus* consts
	DownloadedAt *time.Time `json:"downloaded_at,omitempty"`
	Quality      string     `json:"quality,omitempty"`
	FetchedAt    time.Time  `json:"fetched_at"`
}

// NewEpisodeFromTVDBData maps a raw TVDB episode map to an Episode model.
// Returns nil if the id field is missing or zero.
func NewEpisodeFromTVDBData(showID string, raw map[string]interface{}) *Episode {
	// id is required — bail out if missing or zero
	idFloat, ok := raw["id"].(float64)
	if !ok || idFloat == 0 {
		return nil
	}

	ep := &Episode{
		ID:     fmt.Sprintf("%.0f", idFloat),
		ShowID: showID,
		// Status deliberately left empty — caller sets it based on air date context
		FetchedAt: time.Now(),
	}

	if name, ok := raw["name"].(string); ok {
		ep.Title = name
	}

	if aired, ok := raw["aired"].(string); ok {
		ep.AirDate = aired
	}

	if seasonNum, ok := raw["seasonNumber"].(float64); ok {
		ep.Season = int(seasonNum)
	}

	if number, ok := raw["number"].(float64); ok {
		ep.Number = int(number)
	}

	if overview, ok := raw["overview"].(string); ok {
		ep.Overview = overview
	}

	return ep
}
