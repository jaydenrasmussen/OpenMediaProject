package indexers

import (
	"encoding/xml"
	"fmt"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"
)

const nyaaBaseURL = "https://nyaa.si/?page=rss"

// NyaaIndexer searches Nyaa.si via its RSS feed. Nyaa.si is an anime-focused
// torrent site with a well-maintained public RSS API.
type NyaaIndexer struct {
	enabled bool
	client  *http.Client
}

// NewNyaaIndexer returns a new, enabled NyaaIndexer.
func NewNyaaIndexer() *NyaaIndexer {
	return &NyaaIndexer{
		enabled: true,
		client:  &http.Client{Timeout: 15 * time.Second},
	}
}

func (n *NyaaIndexer) Name() string        { return "Nyaa" }
func (n *NyaaIndexer) Supports() MediaType { return MediaTypeBoth }
func (n *NyaaIndexer) Priority() int       { return 5 }
func (n *NyaaIndexer) Enabled() bool       { return n.enabled }
func (n *NyaaIndexer) SetEnabled(v bool)   { n.enabled = v }

// nyaaFeed is the top-level RSS envelope returned by Nyaa.si.
type nyaaFeed struct {
	Items []nyaaItem `xml:"channel>item"`
}

// nyaaItem is a single result in the Nyaa RSS feed.
// Nyaa extends RSS with a custom namespace (xmlns:nyaa="https://nyaa.si/xmlns/nyaa").
type nyaaItem struct {
	Title       string `xml:"title"`
	Link        string `xml:"link"`
	PubDate     string `xml:"pubDate"`
	MagnetURI   string `xml:"magnetUri"`   // nyaa:magnetUri
	Seeders     string `xml:"seeders"`     // nyaa:seeders
	Leechers    string `xml:"leechers"`    // nyaa:leechers
	Size        string `xml:"size"`        // nyaa:size  e.g. "1.5 GiB"
	InfoHash    string `xml:"infoHash"`    // nyaa:infoHash
}

// Search implements Indexer.
func (n *NyaaIndexer) Search(req *SearchRequest) ([]*SearchResult, error) {
	params := url.Values{}
	params.Set("q", req.Query)
	// c=1_0 = All anime; f=0 = no filter
	params.Set("c", "1_0")
	params.Set("f", "0")

	apiURL := nyaaBaseURL + "&" + params.Encode()

	resp, err := n.client.Get(apiURL)
	if err != nil {
		return nil, fmt.Errorf("nyaa: request failed: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("nyaa: unexpected status %d", resp.StatusCode)
	}

	var feed nyaaFeed
	if err := xml.NewDecoder(resp.Body).Decode(&feed); err != nil {
		return nil, fmt.Errorf("nyaa: decode failed: %w", err)
	}

	var results []*SearchResult
	for _, item := range feed.Items {
		hash := strings.ToLower(item.InfoHash)
		if hash == "" {
			// Fall back to extracting hash from the magnet URI.
			if idx := strings.Index(strings.ToLower(item.MagnetURI), "urn:btih:"); idx != -1 {
				rest := item.MagnetURI[idx+len("urn:btih:"):]
				end := strings.IndexAny(rest, "&")
				if end == -1 {
					hash = strings.ToLower(rest)
				} else {
					hash = strings.ToLower(rest[:end])
				}
			}
		}
		if hash == "" {
			continue
		}

		seeders, _ := strconv.Atoi(item.Seeders)
		leechers, _ := strconv.Atoi(item.Leechers)
		size := parseNyaaSize(item.Size)

		var published time.Time
		if item.PubDate != "" {
			published, _ = time.Parse(time.RFC1123Z, item.PubDate)
			if published.IsZero() {
				published, _ = time.Parse(time.RFC1123, item.PubDate)
			}
		}

		results = append(results, &SearchResult{
			Title:       item.Title,
			InfoHash:    hash,
			MagnetURL:   item.MagnetURI,
			DownloadURL: item.Link + "/download",
			Size:        size,
			Seeders:     seeders,
			Leechers:    leechers,
			Quality:     ParseQuality(item.Title),
			Source:      n.Name(),
			PublishedAt: published,
		})
	}

	return results, nil
}

// parseNyaaSize converts Nyaa size strings ("1.5 GiB", "850 MiB") to bytes.
func parseNyaaSize(s string) int64 {
	s = strings.TrimSpace(s)
	parts := strings.Fields(s)
	if len(parts) < 2 {
		return 0
	}
	val, err := strconv.ParseFloat(parts[0], 64)
	if err != nil {
		return 0
	}
	switch strings.ToUpper(parts[1]) {
	case "TIB", "TB":
		return int64(val * 1099511627776)
	case "GIB", "GB":
		return int64(val * 1073741824)
	case "MIB", "MB":
		return int64(val * 1048576)
	case "KIB", "KB":
		return int64(val * 1024)
	default:
		return int64(val)
	}
}
