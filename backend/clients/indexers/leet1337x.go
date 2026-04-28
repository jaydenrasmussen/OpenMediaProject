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

const leet1337xBaseURL = "https://www.1337x.to"

// Leet1337xIndexer scrapes 1337x for torrents.
type Leet1337xIndexer struct {
	enabled bool
	client  *http.Client
}

// NewLeet1337xIndexer returns a new, enabled Leet1337xIndexer.
func NewLeet1337xIndexer() *Leet1337xIndexer {
	return &Leet1337xIndexer{
		enabled: true,
		client:  &http.Client{Timeout: 20 * time.Second},
	}
}

func (l *Leet1337xIndexer) Name() string        { return "1337x" }
func (l *Leet1337xIndexer) Supports() MediaType { return MediaTypeBoth }
func (l *Leet1337xIndexer) Priority() int       { return 4 }
func (l *Leet1337xIndexer) Enabled() bool       { return l.enabled }
func (l *Leet1337xIndexer) SetEnabled(v bool)   { l.enabled = v }

var (
	leet1337xLinkRe     = regexp.MustCompile(`href="(/torrent/\d+/[^"]+)"`)
	leet1337xTitleRe    = regexp.MustCompile(`<title>([^<]+)</title>`)
	leet1337xMagnetRe   = regexp.MustCompile(`href="(magnet:[^"]+)"`)
	leet1337xSeedersRe  = regexp.MustCompile(`(?i)<dt>\s*Seeders\s*</dt>\s*<dd[^>]*>(\d+)`)
	leet1337xLeechersRe = regexp.MustCompile(`(?i)<dt>\s*Leechers\s*</dt>\s*<dd[^>]*>(\d+)`)
	leet1337xSizeRe     = regexp.MustCompile(`(?i)<dt>\s*Total Size\s*</dt>\s*<dd[^>]*>([^<]+)`)
)

// parseSizeString converts a human-readable size like "2.1 GB" or "850 MB" to bytes.
func parseSizeString(s string) int64 {
	s = strings.TrimSpace(s)
	// Remove non-breaking spaces and similar unicode whitespace.
	s = strings.ReplaceAll(s, "\u00a0", " ")
	parts := strings.Fields(s)
	if len(parts) < 2 {
		return 0
	}
	val, err := strconv.ParseFloat(parts[0], 64)
	if err != nil {
		return 0
	}
	switch strings.ToUpper(parts[1]) {
	case "TB", "TIB":
		return int64(val * 1e12)
	case "GB", "GIB":
		return int64(val * 1e9)
	case "MB", "MIB":
		return int64(val * 1e6)
	case "KB", "KIB":
		return int64(val * 1e3)
	default:
		return int64(val)
	}
}

// fetchHTML performs a GET with a browser-like User-Agent and returns the body.
func (l *Leet1337xIndexer) fetchHTML(rawURL string) (string, error) {
	req, err := http.NewRequest(http.MethodGet, rawURL, nil)
	if err != nil {
		return "", err
	}
	req.Header.Set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
	req.Header.Set("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
	req.Header.Set("Accept-Language", "en-US,en;q=0.5")

	resp, err := l.client.Do(req)
	if err != nil {
		return "", err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return "", fmt.Errorf("1337x: status %d for %s", resp.StatusCode, rawURL)
	}

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return "", err
	}
	return string(body), nil
}

// Search implements Indexer.
func (l *Leet1337xIndexer) Search(req *SearchRequest) ([]*SearchResult, error) {
	searchURL := fmt.Sprintf("%s/search/%s/1/", leet1337xBaseURL, url.PathEscape(req.Query))

	html, err := l.fetchHTML(searchURL)
	if err != nil {
		return nil, fmt.Errorf("1337x: search page failed: %w", err)
	}

	// Collect unique detail-page paths.
	matches := leet1337xLinkRe.FindAllStringSubmatch(html, -1)
	seen := make(map[string]struct{})
	var paths []string
	for _, m := range matches {
		path := m[1]
		if _, ok := seen[path]; ok {
			continue
		}
		seen[path] = struct{}{}
		paths = append(paths, path)
		if len(paths) >= 5 {
			break
		}
	}

	if len(paths) == 0 {
		return nil, nil
	}

	var results []*SearchResult
	for i, path := range paths {
		if i > 0 {
			time.Sleep(500 * time.Millisecond)
		}

		detailURL := leet1337xBaseURL + path
		detailHTML, err := l.fetchHTML(detailURL)
		if err != nil {
			// Skip individual failures.
			continue
		}

		// Extract title.
		title := ""
		if tm := leet1337xTitleRe.FindStringSubmatch(detailHTML); len(tm) == 2 {
			title = strings.TrimSuffix(strings.TrimSpace(tm[1]), " - 1337x")
		}
		if title == "" {
			continue
		}

		// Extract magnet link.
		magnet := ""
		if mm := leet1337xMagnetRe.FindStringSubmatch(detailHTML); len(mm) == 2 {
			magnet = mm[1]
		}
		if magnet == "" {
			continue
		}

		// Extract InfoHash from magnet (xt=urn:btih:<hash>).
		infoHash := ""
		if idx := strings.Index(strings.ToLower(magnet), "urn:btih:"); idx != -1 {
			rest := magnet[idx+len("urn:btih:"):]
			end := strings.IndexAny(rest, "&")
			if end == -1 {
				infoHash = rest
			} else {
				infoHash = rest[:end]
			}
			infoHash = strings.ToLower(infoHash)
		}

		// Extract seeders.
		seeders := 0
		if sm := leet1337xSeedersRe.FindStringSubmatch(detailHTML); len(sm) == 2 {
			seeders, _ = strconv.Atoi(sm[1])
		}

		// Extract leechers.
		leechers := 0
		if lm := leet1337xLeechersRe.FindStringSubmatch(detailHTML); len(lm) == 2 {
			leechers, _ = strconv.Atoi(lm[1])
		}

		// Extract size.
		var sizeBytes int64
		if szm := leet1337xSizeRe.FindStringSubmatch(detailHTML); len(szm) == 2 {
			sizeBytes = parseSizeString(szm[1])
		}

		results = append(results, &SearchResult{
			Title:       title,
			InfoHash:    infoHash,
			MagnetURL:   magnet,
			DownloadURL: "",
			Size:        sizeBytes,
			Seeders:     seeders,
			Leechers:    leechers,
			Quality:     ParseQuality(title),
			Source:      l.Name(),
			PublishedAt: time.Time{},
		})
	}

	return results, nil
}
