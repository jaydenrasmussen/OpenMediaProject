package indexers

import (
	"fmt"
	"io"
	"net/http"
	"net/url"
	"regexp"
	"strconv"
	"strings"
	"time"
)

const magnetdlBaseURL = "https://www.magnetdl.com/search/"

// MagnetdlIndexer scrapes Magnetdl search results.
type MagnetdlIndexer struct {
	enabled bool
	client  *http.Client
}

// NewMagnetdlIndexer returns a new, enabled MagnetdlIndexer.
func NewMagnetdlIndexer() *MagnetdlIndexer {
	return &MagnetdlIndexer{
		enabled: true,
		client:  &http.Client{Timeout: 20 * time.Second},
	}
}

func (m *MagnetdlIndexer) Name() string        { return "Magnetdl" }
func (m *MagnetdlIndexer) Supports() MediaType { return MediaTypeBoth }
func (m *MagnetdlIndexer) Priority() int       { return 7 }
func (m *MagnetdlIndexer) Enabled() bool       { return m.enabled }
func (m *MagnetdlIndexer) SetEnabled(v bool)   { m.enabled = v }

var (
	mdlMagnetRe  = regexp.MustCompile(`href="(magnet:[^"]+)"`)
	mdlTitleRe   = regexp.MustCompile(`class="n"[^>]*><a[^>]*title="([^"]+)"`)
	mdlSeedsRe   = regexp.MustCompile(`class="s">(\d+)<`)
	mdlLeechsRe  = regexp.MustCompile(`class="l">(\d+)<`)
)

func (m *MagnetdlIndexer) fetchHTML(rawURL string) (string, error) {
	req, err := http.NewRequest(http.MethodGet, rawURL, nil)
	if err != nil {
		return "", err
	}
	req.Header.Set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
	req.Header.Set("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")

	resp, err := m.client.Do(req)
	if err != nil {
		return "", err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return "", fmt.Errorf("magnetdl: status %d", resp.StatusCode)
	}

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return "", err
	}
	return string(body), nil
}

// Search implements Indexer.
func (m *MagnetdlIndexer) Search(req *SearchRequest) ([]*SearchResult, error) {
	params := url.Values{}
	params.Set("q", req.Query)

	searchURL := magnetdlBaseURL + "?" + params.Encode()

	html, err := m.fetchHTML(searchURL)
	if err != nil {
		return nil, fmt.Errorf("magnetdl: fetch failed: %w", err)
	}

	magnets := mdlMagnetRe.FindAllStringSubmatch(html, 20)
	titles := mdlTitleRe.FindAllStringSubmatch(html, 20)
	seeds := mdlSeedsRe.FindAllStringSubmatch(html, 20)
	leeches := mdlLeechsRe.FindAllStringSubmatch(html, 20)

	var results []*SearchResult
	for i, mg := range magnets {
		magnet := mg[1]

		hash := ""
		if idx := strings.Index(strings.ToLower(magnet), "urn:btih:"); idx != -1 {
			rest := magnet[idx+len("urn:btih:"):]
			end := strings.IndexAny(rest, "&")
			if end == -1 {
				hash = strings.ToLower(rest)
			} else {
				hash = strings.ToLower(rest[:end])
			}
		}
		if hash == "" {
			continue
		}

		title := ""
		if i < len(titles) {
			title = strings.TrimSpace(titles[i][1])
		}
		if title == "" {
			continue
		}

		seeders := 0
		if i < len(seeds) {
			seeders, _ = strconv.Atoi(seeds[i][1])
		}
		leechers := 0
		if i < len(leeches) {
			leechers, _ = strconv.Atoi(leeches[i][1])
		}

		results = append(results, &SearchResult{
			Title:       title,
			InfoHash:    hash,
			MagnetURL:   magnet,
			Seeders:     seeders,
			Leechers:    leechers,
			Quality:     ParseQuality(title),
			Source:      m.Name(),
			PublishedAt: time.Time{},
		})
	}

	return results, nil
}
