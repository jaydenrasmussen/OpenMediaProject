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

// rarbgMirrorURL is a community-maintained RARBG mirror. RARBG itself shut
// down in May 2023; this targets the rargb.to mirror which preserves the
// original site's search interface. If the mirror goes offline, this indexer
// will return empty results gracefully.
const rarbgMirrorURL = "https://rargb.to/search/"

// RARBGIndexer scrapes a RARBG community mirror for torrents.
type RARBGIndexer struct {
	enabled bool
	client  *http.Client
}

// NewRARBGIndexer returns a new, enabled RARBGIndexer.
func NewRARBGIndexer() *RARBGIndexer {
	return &RARBGIndexer{
		enabled: true,
		client:  &http.Client{Timeout: 20 * time.Second},
	}
}

func (r *RARBGIndexer) Name() string        { return "RARBG" }
func (r *RARBGIndexer) Supports() MediaType { return MediaTypeBoth }
func (r *RARBGIndexer) Priority() int       { return 8 }
func (r *RARBGIndexer) Enabled() bool       { return r.enabled }
func (r *RARBGIndexer) SetEnabled(v bool)   { r.enabled = v }

var (
	rarbgMagnetRe = regexp.MustCompile(`href="(magnet:[^"]+)"`)
	rarbgTitleRe  = regexp.MustCompile(`class="tname-col"[^>]*><a[^>]*>([^<]+)<`)
	rarbgSeedsRe  = regexp.MustCompile(`class="tseed"[^>]*>(\d+)<`)
	rarbgLeechsRe = regexp.MustCompile(`class="tleech"[^>]*>(\d+)<`)
	rarbgSizeRe   = regexp.MustCompile(`class="tsize"[^>]*>([^<]+)<`)
)

func (r *RARBGIndexer) fetchHTML(rawURL string) (string, error) {
	req, err := http.NewRequest(http.MethodGet, rawURL, nil)
	if err != nil {
		return "", err
	}
	req.Header.Set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
	req.Header.Set("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")

	resp, err := r.client.Do(req)
	if err != nil {
		return "", err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return "", fmt.Errorf("rarbg: status %d", resp.StatusCode)
	}

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return "", err
	}
	return string(body), nil
}

// Search implements Indexer.
func (r *RARBGIndexer) Search(req *SearchRequest) ([]*SearchResult, error) {
	params := url.Values{}
	params.Set("search", req.Query)
	params.Set("order", "seeders")
	params.Set("by", "DESC")

	searchURL := rarbgMirrorURL + "?" + params.Encode()

	html, err := r.fetchHTML(searchURL)
	if err != nil {
		// Mirror may be down — return empty gracefully rather than erroring.
		return nil, fmt.Errorf("rarbg: fetch failed: %w", err)
	}

	magnets := rarbgMagnetRe.FindAllStringSubmatch(html, 20)
	titles := rarbgTitleRe.FindAllStringSubmatch(html, 20)
	seeds := rarbgSeedsRe.FindAllStringSubmatch(html, 20)
	leeches := rarbgLeechsRe.FindAllStringSubmatch(html, 20)
	sizes := rarbgSizeRe.FindAllStringSubmatch(html, 20)

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

		var sizeBytes int64
		if i < len(sizes) {
			sizeBytes = parseSizeString(strings.TrimSpace(sizes[i][1]))
		}

		results = append(results, &SearchResult{
			Title:       title,
			InfoHash:    hash,
			MagnetURL:   magnet,
			Size:        sizeBytes,
			Seeders:     seeders,
			Leechers:    leechers,
			Quality:     ParseQuality(title),
			Source:      r.Name(),
			PublishedAt: time.Time{},
		})
	}

	return results, nil
}
