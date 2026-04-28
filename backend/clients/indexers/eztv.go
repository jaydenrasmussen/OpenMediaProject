package indexers

import (
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
	"regexp"
	"strings"
	"time"
)

const eztvBaseURL = "https://eztv.re/api/get-torrents"

// EZTVIndexer searches the EZTV TV torrent API.
type EZTVIndexer struct {
	enabled bool
	client  *http.Client
}

// NewEZTVIndexer returns a new, enabled EZTVIndexer.
func NewEZTVIndexer() *EZTVIndexer {
	return &EZTVIndexer{
		enabled: true,
		client:  &http.Client{Timeout: 15 * time.Second},
	}
}

func (e *EZTVIndexer) Name() string        { return "EZTV" }
func (e *EZTVIndexer) Supports() MediaType { return MediaTypeTV }
func (e *EZTVIndexer) Priority() int       { return 2 }
func (e *EZTVIndexer) Enabled() bool       { return e.enabled }
func (e *EZTVIndexer) SetEnabled(v bool)   { e.enabled = v }

// eztvResponse mirrors the EZTV API envelope.
type eztvResponse struct {
	Torrents []eztvTorrent `json:"torrents"`
}

// eztvTorrent mirrors a single torrent entry from the EZTV API.
// size_bytes and seeds/peers come back as JSON numbers but the spec notes
// size_bytes may be a string in some responses, so we use json.RawMessage
// and parse manually.
type eztvTorrent struct {
	Title            string      `json:"title"`
	Hash             string      `json:"hash"`
	TorrentURL       string      `json:"torrent_url"`
	MagnetURL        string      `json:"magnet_url"`
	SizeBytes        json.Number `json:"size_bytes"`
	Seeds            int         `json:"seeds"`
	Peers            int         `json:"peers"`
	DateReleasedUnix int64       `json:"date_released_unix"`
}

// seasonEpisodeRegexp matches patterns like S01E02 or S01 in a title.
var (
	eztvSERegexp = regexp.MustCompile(`(?i)S(\d{1,2})E(\d{1,2})`)
	eztvSRegexp  = regexp.MustCompile(`(?i)S(\d{1,2})`)
)

// matchesSE reports whether the torrent title matches the requested
// season / episode constraints (0 means "any").
func matchesSE(title string, season, episode int) bool {
	if season == 0 {
		// No season filter — accept everything.
		return true
	}

	upper := strings.ToUpper(title)

	if episode != 0 {
		// Must match S<NN>E<NN> exactly.
		matches := eztvSERegexp.FindAllStringSubmatch(upper, -1)
		for _, m := range matches {
			s, e2 := 0, 0
			fmt.Sscanf(m[1], "%d", &s)
			fmt.Sscanf(m[2], "%d", &e2)
			if s == season && e2 == episode {
				return true
			}
		}
		return false
	}

	// Season-only filter: accept any episode in the season, or whole-season
	// packs that only mention S<NN> without an episode number.
	seMatches := eztvSERegexp.FindAllStringSubmatch(upper, -1)
	for _, m := range seMatches {
		s := 0
		fmt.Sscanf(m[1], "%d", &s)
		if s == season {
			return true
		}
	}

	// Also accept season packs: S<NN> without E<NN>.
	sMatches := eztvSRegexp.FindAllStringSubmatch(upper, -1)
	for _, m := range sMatches {
		s := 0
		fmt.Sscanf(m[1], "%d", &s)
		if s == season {
			// Make sure it is not a full S01E01 match (already handled above).
			if !eztvSERegexp.MatchString(upper) {
				return true
			}
		}
	}

	return false
}

// Search implements Indexer.
func (e *EZTVIndexer) Search(req *SearchRequest) ([]*SearchResult, error) {
	params := url.Values{}
	params.Set("limit", "100")
	params.Set("page", "1")

	if req.ImdbID != "" {
		// Strip the "tt" prefix to get the bare numeric ID.
		numeric := strings.TrimPrefix(req.ImdbID, "tt")
		params.Set("imdb_id", numeric)
	}

	apiURL := eztvBaseURL + "?" + params.Encode()

	resp, err := e.client.Get(apiURL)
	if err != nil {
		return nil, fmt.Errorf("eztv: request failed: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("eztv: unexpected status %d", resp.StatusCode)
	}

	var apiResp eztvResponse
	if err := json.NewDecoder(resp.Body).Decode(&apiResp); err != nil {
		return nil, fmt.Errorf("eztv: decode failed: %w", err)
	}

	// Client-side title filter when no IMDB ID is provided and a query is given.
	if req.Query != "" && req.ImdbID == "" {
		filtered := apiResp.Torrents[:0]
		for _, t := range apiResp.Torrents {
			if strings.Contains(strings.ToLower(t.Title), strings.ToLower(req.Query)) {
				filtered = append(filtered, t)
			}
		}
		apiResp.Torrents = filtered
	}

	var results []*SearchResult
	for _, t := range apiResp.Torrents {
		if t.Hash == "" {
			continue
		}

		// Apply season/episode filter when requested.
		if !matchesSE(t.Title, req.Season, req.Episode) {
			continue
		}

		var sizeBytes int64
		if t.SizeBytes != "" {
			sizeBytes, _ = t.SizeBytes.Int64()
		}

		var published time.Time
		if t.DateReleasedUnix != 0 {
			published = time.Unix(t.DateReleasedUnix, 0)
		}

		results = append(results, &SearchResult{
			Title:       t.Title,
			InfoHash:    t.Hash,
			MagnetURL:   t.MagnetURL,
			DownloadURL: t.TorrentURL,
			Size:        sizeBytes,
			Seeders:     t.Seeds,
			Leechers:    t.Peers,
			Quality:     ParseQuality(t.Title),
			Source:      e.Name(),
			PublishedAt: published,
		})
	}

	return results, nil
}
