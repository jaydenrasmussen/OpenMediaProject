package indexers

import (
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
	"regexp"
	"strconv"
	"strings"
	"time"
)

const pirateBayBaseURL = "https://apibay.org/q.php"

// PirateBayIndexer searches the Pirate Bay via the apibay.org JSON API.
type PirateBayIndexer struct {
	enabled bool
	client  *http.Client
}

// NewPirateBayIndexer returns a new, enabled PirateBayIndexer.
func NewPirateBayIndexer() *PirateBayIndexer {
	return &PirateBayIndexer{
		enabled: true,
		client:  &http.Client{Timeout: 15 * time.Second},
	}
}

func (p *PirateBayIndexer) Name() string        { return "PirateBay" }
func (p *PirateBayIndexer) Supports() MediaType { return MediaTypeBoth }
func (p *PirateBayIndexer) Priority() int       { return 3 }
func (p *PirateBayIndexer) Enabled() bool       { return p.enabled }
func (p *PirateBayIndexer) SetEnabled(v bool)   { p.enabled = v }

type pirateBayTorrent struct {
	ID       string `json:"id"`
	Name     string `json:"name"`
	InfoHash string `json:"info_hash"`
	Leechers string `json:"leechers"`
	Seeders  string `json:"seeders"`
	NumFiles string `json:"num_files"`
	Size     string `json:"size"`
	Username string `json:"username"`
	Added    string `json:"added"`
	Status   string `json:"status"`
	Category string `json:"category"`
	IMDB     string `json:"imdb"`
}

func (p *PirateBayIndexer) Search(req *SearchRequest) ([]*SearchResult, error) {
	query := req.Query
	if req.MediaType == MediaTypeTV && req.Season != 0 {
		alreadyHasSE := regexp.MustCompile(`(?i)S\d{2}E\d{2}`).MatchString(query)
		alreadyHasS := regexp.MustCompile(`(?i)S\d{2}`).MatchString(query)
		if req.Episode != 0 {
			if !alreadyHasSE {
				query = fmt.Sprintf("%s S%02dE%02d", query, req.Season, req.Episode)
			}
		} else {
			if !alreadyHasS {
				query = fmt.Sprintf("%s S%02d", query, req.Season)
			}
		}
	}
	params := url.Values{}
	params.Set("q", query)
	params.Set("cat", "200")
	apiURL := pirateBayBaseURL + "?" + params.Encode()
	resp, err := p.client.Get(apiURL)
	if err != nil {
		return nil, fmt.Errorf("piratebay: request failed: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("piratebay: unexpected status %d", resp.StatusCode)
	}
	var torrents []pirateBayTorrent
	if err := json.NewDecoder(resp.Body).Decode(&torrents); err != nil {
		return nil, fmt.Errorf("piratebay: decode failed: %w", err)
	}
	if len(torrents) == 1 && strings.EqualFold(strings.TrimRight(torrents[0].Name, ". "), "No results returned") {
		return nil, nil
	}
	trackers := "&tr=udp://tracker.openbittorrent.com:80&tr=udp://open.demonii.com:1337"
	var results []*SearchResult
	for _, t := range torrents {
		if t.InfoHash == "" {
			continue
		}
		seeders, _ := strconv.Atoi(t.Seeders)
		leechers, _ := strconv.Atoi(t.Leechers)
		size, _ := strconv.ParseInt(t.Size, 10, 64)
		var published time.Time
		if ts, err2 := strconv.ParseInt(t.Added, 10, 64); err2 == nil && ts > 0 {
			published = time.Unix(ts, 0)
		}
		magnet := fmt.Sprintf("magnet:?xt=urn:btih:%s&dn=%s%s", t.InfoHash, url.QueryEscape(t.Name), trackers)
		results = append(results, &SearchResult{
			Title:       t.Name,
			InfoHash:    t.InfoHash,
			MagnetURL:   magnet,
			DownloadURL: "",
			Size:        size,
			Seeders:     seeders,
			Leechers:    leechers,
			Quality:     ParseQuality(t.Name),
			Source:      p.Name(),
			PublishedAt: published,
		})
	}
	return results, nil
}
