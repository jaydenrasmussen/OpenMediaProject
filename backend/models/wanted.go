package models

import "time"

// WantedItem status constants
const (
	WantedStatusPending     = "pending"      // submitted by user, awaiting admin approval
	WantedStatusRejected    = "rejected"     // rejected by admin
	WantedStatusWanted      = "wanted"
	WantedStatusDownloading = "downloading"
	WantedStatusAvailable   = "available"
	WantedStatusUnavailable = "unavailable"
)

// WantedItem represents a piece of media a user wants to download
type WantedItem struct {
	ID           string    `json:"id"`
	MediaType    string    `json:"media_type"` // "movie" | "tv"
	MediaID      string    `json:"media_id"`   // TMDB or TVDB ID
	Title        string    `json:"title"`
	Season       int       `json:"season,omitempty"`        // 0 = whole show / movie
	Episode      int       `json:"episode,omitempty"`       // 0 = whole season
	MinQuality   string    `json:"min_quality"`             // minimum acceptable quality, e.g. "720p"
	UpgradeUntil string    `json:"upgrade_until,omitempty"` // e.g. "1080p"; empty = no upgrade
	AddedAt      time.Time `json:"added_at"`
	UpdatedAt    time.Time `json:"updated_at"`
	Status       string    `json:"status"`
	AddedBy      string    `json:"added_by,omitempty"`
}
