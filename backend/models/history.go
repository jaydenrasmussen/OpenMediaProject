package models

import (
	"encoding/json"
	"time"
)

// HistoryEventType categorises the kind of event recorded in history.
type HistoryEventType string

const (
	HistoryEventTorrentAdded    HistoryEventType = "torrent_added"
	HistoryEventDownloadComplete HistoryEventType = "download_complete"
	HistoryEventDownloadFailed  HistoryEventType = "download_failed"
	HistoryEventUpgradeFound    HistoryEventType = "upgrade_found"
	HistoryEventSearchStarted   HistoryEventType = "search_started"
)

// HistoryEntry records a single activity event.
type HistoryEntry struct {
	ID        string           `json:"id"`
	EventType HistoryEventType `json:"event_type"`
	Title     string           `json:"title"`
	MediaType string           `json:"media_type,omitempty"` // "movie" | "tv" | ""
	MediaID   string           `json:"media_id,omitempty"`
	Message   string           `json:"message,omitempty"`
	User      string           `json:"user,omitempty"`
	CreatedAt time.Time        `json:"created_at"`
}

// ToJSON serialises the entry.
func (h *HistoryEntry) ToJSON() ([]byte, error) {
	return json.Marshal(h)
}

// FromJSON deserialises the entry.
func (h *HistoryEntry) FromJSON(data []byte) error {
	return json.Unmarshal(data, h)
}

// GetKey returns the NutsDB storage key.
func (h *HistoryEntry) GetKey() []byte {
	return []byte(h.ID)
}
