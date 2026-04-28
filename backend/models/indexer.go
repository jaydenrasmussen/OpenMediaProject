package models

import "time"

// IndexerType represents the protocol an indexer uses
type IndexerType string

const (
	IndexerTypeTorznab IndexerType = "torznab" // Prowlarr, Jackett (torrent)
	IndexerTypeNewznab IndexerType = "newznab" // Usenet (future use)
)

// Indexer represents a configured torrent indexer
type Indexer struct {
	ID       string      `json:"id"`
	Name     string      `json:"name"`
	Type     IndexerType `json:"type"`
	URL      string      `json:"url"` // Base URL, e.g. http://prowlarr:9696/1/
	APIKey   string      `json:"api_key"`
	Enabled  bool        `json:"enabled"`
	Priority int         `json:"priority"` // Lower = higher priority
	AddedAt  time.Time   `json:"added_at"`
}

// IndexerSearchResult represents a single result returned from an indexer search
type IndexerSearchResult struct {
	Title       string    `json:"title"`
	GUID        string    `json:"guid"`
	Link        string    `json:"link"` // Download URL or magnet
	Size        int64     `json:"size"` // In bytes
	PublishedAt time.Time `json:"published_at"`
	Seeders     int       `json:"seeders"`
	Leechers    int       `json:"leechers"`
	InfoHash    string    `json:"info_hash"`
	Category    []string  `json:"category"`
	Indexer     string    `json:"indexer"`      // Name of the originating indexer
	DownloadURL string    `json:"download_url"` // Direct .torrent download URL
	MagnetURL   string    `json:"magnet_url"`
	ImdbID      string    `json:"imdb_id,omitempty"`
	TvdbID      string    `json:"tvdb_id,omitempty"`
	Season      int       `json:"season,omitempty"`
	Episode     int       `json:"episode,omitempty"`
}

// IndexerSearchRequest represents a search request to one or more indexers
type IndexerSearchRequest struct {
	Query     string `json:"query"`
	Type      string `json:"type"` // "movie", "tv", "search"
	ImdbID    string `json:"imdb_id,omitempty"`
	TvdbID    string `json:"tvdb_id,omitempty"`
	Season    int    `json:"season,omitempty"`
	Episode   int    `json:"episode,omitempty"`
	IndexerID string `json:"indexer_id,omitempty"` // Empty = all enabled indexers
	Limit     int    `json:"limit,omitempty"`      // Max results per indexer (default 20)
}

// GrabRequest represents a request to grab a torrent from an indexer result and send it to the download client
type GrabRequest struct {
	DownloadURL string `json:"download_url"` // .torrent URL or magnet
	Title       string `json:"title"`
	IndexerID   string `json:"indexer_id,omitempty"`
}
