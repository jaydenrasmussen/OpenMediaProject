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

// RSSIndexer parses any RSS/Atom feed that contains torrent entries.
// It understands both plain RSS (with <enclosure> for download URLs) and
// Torznab-compatible feeds (with <torznab:attr name="magneturl"> etc.).
//
// URLTemplate is a Go format string where %s is replaced by the URL-encoded
// search query, e.g. "https://example.com/rss?q=%s".
//
// RSSIndexer is not registered in NewDefaultRegistry because it requires a
// caller-supplied URL. Instantiate it directly and call registry.Register().
type RSSIndexer struct {
	name        string
	urlTemplate string
	mediaType   MediaType
	priority    int
	enabled     bool
	client      *http.Client
}

// NewRSSIndexer creates a configurable RSS indexer.
// urlTemplate must contain a single %s verb where the search query will be
// substituted, e.g. "https://example.com/rss?q=%s&cat=0".
func NewRSSIndexer(name, urlTemplate string, mediaType MediaType, priority int) *RSSIndexer {
	return &RSSIndexer{
		name:        name,
		urlTemplate: urlTemplate,
		mediaType:   mediaType,
		priority:    priority,
		enabled:     true,
		client:      &http.Client{Timeout: 15 * time.Second},
	}
}

func (r *RSSIndexer) Name() string        { return r.name }
func (r *RSSIndexer) Supports() MediaType { return r.mediaType }
func (r *RSSIndexer) Priority() int       { return r.priority }
func (r *RSSIndexer) Enabled() bool       { return r.enabled }
func (r *RSSIndexer) SetEnabled(v bool)   { r.enabled = v }

// rssFeed is a minimal RSS/Atom envelope covering the fields we care about.
type rssFeed struct {
	Items []rssItem `xml:"channel>item"`
}

type rssItem struct {
	Title      string        `xml:"title"`
	Link       string        `xml:"link"`
	PubDate    string        `xml:"pubDate"`
	Enclosures []rssEncl     `xml:"enclosure"`
	TorznabAttrs []torznabAttr `xml:"attr"`
	// Nyaa-style fields (reused by some feeds)
	MagnetURI string `xml:"magnetUri"`
	Seeders   string `xml:"seeders"`
	Leechers  string `xml:"leechers"`
	InfoHash  string `xml:"infoHash"`
}

type rssEncl struct {
	URL  string `xml:"url,attr"`
	Type string `xml:"type,attr"`
}

type torznabAttr struct {
	Name  string `xml:"name,attr"`
	Value string `xml:"value,attr"`
}

// Search implements Indexer.
func (r *RSSIndexer) Search(req *SearchRequest) ([]*SearchResult, error) {
	feedURL := fmt.Sprintf(r.urlTemplate, url.QueryEscape(req.Query))

	resp, err := r.client.Get(feedURL)
	if err != nil {
		return nil, fmt.Errorf("rss(%s): request failed: %w", r.name, err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("rss(%s): unexpected status %d", r.name, resp.StatusCode)
	}

	var feed rssFeed
	if err := xml.NewDecoder(resp.Body).Decode(&feed); err != nil {
		return nil, fmt.Errorf("rss(%s): decode failed: %w", r.name, err)
	}

	var results []*SearchResult
	for _, item := range feed.Items {
		res := rssItemToResult(item, r.Name())
		if res != nil {
			results = append(results, res)
		}
	}
	return results, nil
}

// rssItemToResult converts a parsed RSS item to a SearchResult.
// It handles plain RSS with <enclosure>, Torznab attrs, and Nyaa-style fields.
func rssItemToResult(item rssItem, source string) *SearchResult {
	// Collect torznab attributes into a map for easy lookup.
	tzAttrs := make(map[string]string, len(item.TorznabAttrs))
	for _, a := range item.TorznabAttrs {
		tzAttrs[a.Name] = a.Value
	}

	// Resolve magnet URL (priority: nyaa field > torznab attr > enclosure).
	magnet := item.MagnetURI
	if magnet == "" {
		magnet = tzAttrs["magneturl"]
	}
	if magnet == "" {
		for _, enc := range item.Enclosures {
			if strings.HasPrefix(enc.URL, "magnet:") {
				magnet = enc.URL
				break
			}
		}
	}

	// Resolve download URL.
	downloadURL := ""
	for _, enc := range item.Enclosures {
		if strings.Contains(enc.Type, "torrent") || strings.HasSuffix(enc.URL, ".torrent") {
			downloadURL = enc.URL
			break
		}
	}
	if downloadURL == "" && item.Link != "" && (strings.HasSuffix(item.Link, ".torrent") || strings.Contains(item.Link, "/download/")) {
		downloadURL = item.Link
	}

	// Resolve InfoHash.
	hash := strings.ToLower(item.InfoHash)
	if hash == "" {
		hash = strings.ToLower(tzAttrs["infohash"])
	}
	if hash == "" && magnet != "" {
		if idx := strings.Index(strings.ToLower(magnet), "urn:btih:"); idx != -1 {
			rest := magnet[idx+len("urn:btih:"):]
			end := strings.IndexAny(rest, "&")
			if end == -1 {
				hash = strings.ToLower(rest)
			} else {
				hash = strings.ToLower(rest[:end])
			}
		}
	}

	if hash == "" && magnet == "" && downloadURL == "" {
		return nil
	}

	// Seeders / leechers.
	seeders, _ := strconv.Atoi(item.Seeders)
	if seeders == 0 {
		seeders, _ = strconv.Atoi(tzAttrs["seeders"])
	}
	leechers, _ := strconv.Atoi(item.Leechers)
	if leechers == 0 {
		leechers, _ = strconv.Atoi(tzAttrs["peers"])
	}

	// Size.
	var sizeBytes int64
	if s := tzAttrs["size"]; s != "" {
		sizeBytes, _ = strconv.ParseInt(s, 10, 64)
	}

	// Published date.
	var published time.Time
	if item.PubDate != "" {
		published, _ = time.Parse(time.RFC1123Z, item.PubDate)
		if published.IsZero() {
			published, _ = time.Parse(time.RFC1123, item.PubDate)
		}
	}

	return &SearchResult{
		Title:       item.Title,
		InfoHash:    hash,
		MagnetURL:   magnet,
		DownloadURL: downloadURL,
		Size:        sizeBytes,
		Seeders:     seeders,
		Leechers:    leechers,
		Quality:     ParseQuality(item.Title),
		Source:      source,
		PublishedAt: published,
	}
}
