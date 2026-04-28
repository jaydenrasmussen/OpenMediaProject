package models

import "time"

// DownloadStatus values
const (
	DownloadStatusPending     = "pending"
	DownloadStatusDownloading = "downloading"
	DownloadStatusCompleted   = "completed"
	DownloadStatusFailed      = "failed"
)

// PendingDownload tracks an in-progress or completed download
type PendingDownload struct {
	Hash         string    `json:"hash"`
	MediaType    string    `json:"media_type"` // "movie" or "tv"
	MediaID      string    `json:"media_id"`   // TMDB or TVDB ID
	Title        string    `json:"title"`
	Quality      string    `json:"quality"`
	Source       string    `json:"source"` // which indexer found it
	AddedAt      time.Time `json:"added_at"`
	UpdatedAt    time.Time `json:"updated_at"`
	Status       string    `json:"status"` // see DownloadStatus* consts
	AddedBy      string    `json:"added_by,omitempty"`
	Error        string    `json:"error,omitempty"`
	UpgradeUntil string    `json:"upgrade_until,omitempty"`
}
