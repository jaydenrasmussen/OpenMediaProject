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

const torrentGalaxyBaseURL = "https://torrentgalaxy.to/torrents.php"

// TorrentGalaxyIndexer scrapes TorrentGalaxy search results.
type TorrentGalaxyIndexer struct {
	enabled bool
	client  *http.Client
}

// NewTorrentGalaxyIndexer returns a new, enabled TorrentGalaxyIndexer.
func NewTorrentGalaxyIndexer() *TorrentGalaxyIndexer {
	return &TorrentGalaxyIndexer{
		enabled: true,
		client:  &http.Client{Timeout: 20 * time.Second},
	}
}

func (t *TorrentGalaxyIndexer) Name() string        { return "TorrentGalaxy" }
func (t *TorrentGalaxyIndexer) Supports() MediaType { return MediaTypeBoth }
func (t *TorrentGalaxyIndexer) Priority() int       { return 6 }
func (t *TorrentGalaxyIndexer) Enabled() bool       { return t.enabled }
func (t *TorrentGalaxyIndexer) SetEnabled(v bool)   { t.enabled = v }

var (
	tgMagnetRe  = regexp.MustCompile(`href="(magnet:[^"]+)"`)
	tgTitleRe   = regexp.MustCompile(`class="txlight"[^>]*>([^<]+)<`)
	tgSeedRe    = regexp.MustCompile(`class="tul"[^>]*>(\d+)<`)
	tgLeechRe   = regexp.MustCompile(`class="tdl"[^>]*>(\d+)<`)
	tgSizeRe    = regexp.MustCompile(`<span[^>]*>(\d+(?:\.\d+)?)\s*(TB|GB|MB|KB|B)</span>`)
)

func (t *TorrentGalaxyIndexer) fetchHTML(rawURL string) (string, error) {
	req, err := http.NewRequest(http.MethodGet, rawURL, nil)
	if err != nil {
		return "", err
	}
	req.Header.Set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
	req.Header.Set("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")

	resp, err := t.client.Do(req)
	if err != nil {
		return "", err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return "", fmt.Errorf("torrentgalaxy: status %d", resp.StatusCode)
	}

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return "", err
	}
	return string(body), nil
}

// Search implements Indexer.
func (t *TorrentGalaxyIndexer) Search(req *SearchRequest) ([]*SearchResult, error) {
	params := url.Values{}
	params.Set("search", req.Query)
	params.Set("sort", "seeders")
	params.Set("order", "desc")
	params.Set("nox", "1") // no external redirects

	searchURL := torrentGalaxyBaseURL + "?" + params.Encode()

	html, err := t.fetchHTML(searchURL)
	if err != nil {
		return nil, fmt.Errorf("torrentgalaxy: fetch failed: %w", err)
	}

	magnets := tgMagnetRe.FindAllStringSubmatch(html, 20)
	titles := tgTitleRe.FindAllStringSubmatch(html, 20)
	seeds := tgSeedRe.FindAllStringSubmatch(html, 20)
	leeches := tgLeechRe.FindAllStringSubmatch(html, 20)
	sizes := tgSizeRe.FindAllStringSubmatch(html, 20)

	var results []*SearchResult
	for i, m := range magnets {
		magnet := m[1]

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
			val, _ := strconv.ParseFloat(sizes[i][1], 64)
			sizeBytes = tgParseSize(val, sizes[i][2])
		}

		results = append(results, &SearchResult{
			Title:       title,
			InfoHash:    hash,
			MagnetURL:   magnet,
			Size:        sizeBytes,
			Seeders:     seeders,
			Leechers:    leechers,
			Quality:     ParseQuality(title),
			Source:      t.Name(),
			PublishedAt: time.Time{},
		})
	}

	return results, nil
}

func tgParseSize(val float64, unit string) int64 {
	switch strings.ToUpper(unit) {
	case "TB":
		return int64(val * 1e12)
	case "GB":
		return int64(val * 1e9)
	case "MB":
		return int64(val * 1e6)
	case "KB":
		return int64(val * 1e3)
	default:
		return int64(val)
	}
}
