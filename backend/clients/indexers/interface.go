package indexers

import "time"

// MediaType indicates what kind of content an indexer supports
type MediaType string

const (
	MediaTypeMovie MediaType = "movie"
	MediaTypeTV    MediaType = "tv"
	MediaTypeBoth  MediaType = "both"
)

// Quality represents a video quality tier
type Quality string

const (
	Quality2160p Quality = "2160p"
	Quality1080p Quality = "1080p"
	Quality720p  Quality = "720p"
	Quality480p  Quality = "480p"
	QualityAny   Quality = "any"
)

// QualityFallbackOrder is the default preference order
var QualityFallbackOrder = []Quality{Quality1080p, Quality720p, Quality480p, QualityAny}

// SearchRequest is the unified search input for all indexers
type SearchRequest struct {
	Query     string // Free-text title
	MediaType MediaType
	ImdbID    string  // e.g. "tt0903747" — used by movie indexers
	TvdbID    string  // used by TV indexers
	Season    int     // 0 = whole show
	Episode   int     // 0 = whole season
	Quality   Quality // desired quality tier (used for scoring, not filtering)
	Year      int     // release year, for disambiguation
}

// SearchResult is a single normalised result from any indexer
type SearchResult struct {
	Title       string
	InfoHash    string
	MagnetURL   string
	DownloadURL string // .torrent file URL
	Size        int64  // bytes
	Seeders     int
	Leechers    int
	Quality     Quality // parsed from title
	Source      string  // name of the indexer that returned this
	PublishedAt time.Time
}

// Indexer is the interface every built-in provider must implement
type Indexer interface {
	// Name returns the human-readable name of this indexer
	Name() string

	// Supports returns the media type(s) this indexer can search
	Supports() MediaType

	// Priority returns the indexer's default priority (lower = tried first for scoring)
	Priority() int

	// Search executes a search and returns results
	Search(req *SearchRequest) ([]*SearchResult, error)

	// Enabled returns whether this indexer is active
	Enabled() bool

	// SetEnabled toggles the indexer on/off
	SetEnabled(bool)
}
