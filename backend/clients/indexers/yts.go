package indexers

import (
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
	"time"
)

const ytsBaseURL = "https://yts.mx/api/v2/list_movies.json"

// YTSIndexer searches the YTS movie API.
type YTSIndexer struct {
	enabled bool
	client  *http.Client
}

// NewYTSIndexer returns a new, enabled YTSIndexer.
func NewYTSIndexer() *YTSIndexer {
	return &YTSIndexer{
		enabled: true,
		client:  &http.Client{Timeout: 15 * time.Second},
	}
}

func (y *YTSIndexer) Name() string        { return "YTS" }
func (y *YTSIndexer) Supports() MediaType { return MediaTypeMovie }
func (y *YTSIndexer) Priority() int       { return 1 }
func (y *YTSIndexer) Enabled() bool       { return y.enabled }
func (y *YTSIndexer) SetEnabled(v bool)   { y.enabled = v }

// ytsResponse mirrors the top-level YTS API envelope.
type ytsResponse struct {
	Data struct {
		Movies []ytsMovie `json:"movies"`
	} `json:"data"`
}

type ytsMovie struct {
	Title    string       `json:"title"`
	Year     int          `json:"year"`
	Torrents []ytsTorrent `json:"torrents"`
}

type ytsTorrent struct {
	Hash         string `json:"hash"`
	Quality      string `json:"quality"`
	Seeds        int    `json:"seeds"`
	Peers        int    `json:"peers"`
	SizeBytes    int64  `json:"size_bytes"`
	DateUploaded string `json:"date_uploaded"`
}

// Search implements Indexer.
func (y *YTSIndexer) Search(req *SearchRequest) ([]*SearchResult, error) {
	params := url.Values{}

	// Prefer IMDB ID for precision; fall back to free-text title.
	if req.ImdbID != "" {
		params.Set("query_term", req.ImdbID)
	} else {
		params.Set("query_term", req.Query)
	}

	params.Set("limit", "20")
	params.Set("sort_by", "seeds")

	apiURL := ytsBaseURL + "?" + params.Encode()

	resp, err := y.client.Get(apiURL)
	if err != nil {
		return nil, fmt.Errorf("yts: request failed: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("yts: unexpected status %d", resp.StatusCode)
	}

	var apiResp ytsResponse
	if err := json.NewDecoder(resp.Body).Decode(&apiResp); err != nil {
		return nil, fmt.Errorf("yts: decode failed: %w", err)
	}

	trackers := "&tr=udp://open.demonii.com:1337&tr=udp://tracker.openbittorrent.com:80"

	var results []*SearchResult
	for _, movie := range apiResp.Data.Movies {
		for _, t := range movie.Torrents {
			if t.Hash == "" {
				continue
			}

			magnet := fmt.Sprintf(
				"magnet:?xt=urn:btih:%s&dn=%s%s",
				t.Hash,
				url.QueryEscape(movie.Title),
				trackers,
			)

			var published time.Time
			if t.DateUploaded != "" {
				// YTS uses "2006-01-02 15:04:05" layout.
				published, _ = time.Parse("2006-01-02 15:04:05", t.DateUploaded)
			}

			results = append(results, &SearchResult{
				Title:       fmt.Sprintf("%s (%d) [%s]", movie.Title, movie.Year, t.Quality),
				InfoHash:    t.Hash,
				MagnetURL:   magnet,
				DownloadURL: "",
				Size:        t.SizeBytes,
				Seeders:     t.Seeds,
				Leechers:    t.Peers,
				Quality:     ParseQuality(t.Quality),
				Source:      y.Name(),
				PublishedAt: published,
			})
		}
	}

	return results, nil
}
