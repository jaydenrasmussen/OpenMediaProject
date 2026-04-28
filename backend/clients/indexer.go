package clients

import (
	"encoding/xml"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"

	"backend/models"
)

// IndexerClient talks to a single Torznab/Newznab indexer endpoint
type IndexerClient struct {
	indexer    *models.Indexer
	httpClient *http.Client
}

// NewIndexerClient creates a new IndexerClient
func NewIndexerClient(indexer *models.Indexer) *IndexerClient {
	return &IndexerClient{
		indexer:    indexer,
		httpClient: NewHTTPClient(),
	}
}

// Search performs a Torznab/Newznab search and returns parsed results
func (c *IndexerClient) Search(req *models.IndexerSearchRequest) ([]*models.IndexerSearchResult, error) {
	searchURL, err := c.buildSearchURL(req)
	if err != nil {
		return nil, fmt.Errorf("indexer %q: failed to build search URL: %w", c.indexer.Name, err)
	}

	resp, err := c.httpClient.Get(searchURL)
	if err != nil {
		return nil, fmt.Errorf("indexer %q: HTTP request failed: %w", c.indexer.Name, err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("indexer %q: unexpected status code %d", c.indexer.Name, resp.StatusCode)
	}

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("indexer %q: failed to read response body: %w", c.indexer.Name, err)
	}

	results, err := parseFeed(body, c.indexer.Name)
	if err != nil {
		return nil, fmt.Errorf("indexer %q: failed to parse feed: %w", c.indexer.Name, err)
	}

	return results, nil
}

// buildSearchURL constructs the query URL for a Torznab search
func (c *IndexerClient) buildSearchURL(req *models.IndexerSearchRequest) (string, error) {
	base := strings.TrimRight(c.indexer.URL, "/")
	if base == "" {
		return "", fmt.Errorf("indexer URL is empty")
	}

	params := url.Values{}
	params.Set("apikey", c.indexer.APIKey)

	switch req.Type {
	case "movie":
		params.Set("t", "movie")
		if req.ImdbID != "" {
			// Strip leading "tt" prefix if present
			imdbID := strings.TrimPrefix(req.ImdbID, "tt")
			params.Set("imdbid", imdbID)
		}
	case "tv":
		params.Set("t", "tvsearch")
		if req.TvdbID != "" {
			params.Set("tvdbid", req.TvdbID)
		}
		if req.Season != 0 {
			params.Set("season", strconv.Itoa(req.Season))
		}
		if req.Episode != 0 {
			params.Set("ep", strconv.Itoa(req.Episode))
		}
	default:
		params.Set("t", "search")
	}

	params.Set("q", req.Query)

	limit := req.Limit
	if limit <= 0 {
		limit = 20
	}
	params.Set("limit", strconv.Itoa(limit))

	return base + "?" + params.Encode(), nil
}

// ---- XML unmarshalling structs ----

type rssRoot struct {
	XMLName xml.Name   `xml:"rss"`
	Channel rssChannel `xml:"channel"`
}

type rssChannel struct {
	Items []rssItem `xml:"item"`
}

type rssItem struct {
	Title     string       `xml:"title"`
	GUID      string       `xml:"guid"`
	Link      string       `xml:"link"`
	PubDate   string       `xml:"pubDate"`
	Size      int64        `xml:"size"`
	Enclosure rssEnclosure `xml:"enclosure"`

	// Torznab and Newznab attribute elements share the same local name "attr"
	// but live in different namespaces. We capture both with a single slice
	// by matching on the local name via a custom UnmarshalXML below.
	Attrs []rssAttr
}

type rssEnclosure struct {
	URL    string `xml:"url,attr"`
	Length int64  `xml:"length,attr"`
	Type   string `xml:"type,attr"`
}

type rssAttr struct {
	Name  string
	Value string
}

// UnmarshalXML implements xml.Unmarshaler for rssItem so we can capture
// both torznab:attr and newznab:attr elements regardless of namespace.
func (item *rssItem) UnmarshalXML(d *xml.Decoder, start xml.StartElement) error {
	// Use a local alias to avoid infinite recursion.
	type plain struct {
		Title     string       `xml:"title"`
		GUID      string       `xml:"guid"`
		Link      string       `xml:"link"`
		PubDate   string       `xml:"pubDate"`
		Size      int64        `xml:"size"`
		Enclosure rssEnclosure `xml:"enclosure"`
	}

	var p plain

	// Manually walk the token stream so we can intercept attr elements.
	for {
		tok, err := d.Token()
		if err != nil {
			return err
		}

		switch t := tok.(type) {
		case xml.StartElement:
			if t.Name.Local == "attr" {
				// torznab:attr or newznab:attr
				var name, value string
				for _, a := range t.Attr {
					switch a.Name.Local {
					case "name":
						name = a.Value
					case "value":
						value = a.Value
					}
				}
				if name != "" {
					item.Attrs = append(item.Attrs, rssAttr{Name: name, Value: value})
				}
				// consume the closing tag
				if err := d.Skip(); err != nil {
					return err
				}
			} else if t.Name.Local == "enclosure" {
				for _, a := range t.Attr {
					switch a.Name.Local {
					case "url":
						p.Enclosure.URL = a.Value
					case "length":
						p.Enclosure.Length, _ = strconv.ParseInt(a.Value, 10, 64)
					case "type":
						p.Enclosure.Type = a.Value
					}
				}
				if err := d.Skip(); err != nil {
					return err
				}
			} else {
				// Decode into the plain struct fields we know about.
				switch t.Name.Local {
				case "title":
					d.DecodeElement(&p.Title, &t)
				case "guid":
					d.DecodeElement(&p.GUID, &t)
				case "link":
					d.DecodeElement(&p.Link, &t)
				case "pubDate":
					d.DecodeElement(&p.PubDate, &t)
				case "size":
					d.DecodeElement(&p.Size, &t)
				default:
					d.Skip()
				}
			}

		case xml.EndElement:
			// End of <item>
			item.Title = p.Title
			item.GUID = p.GUID
			item.Link = p.Link
			item.PubDate = p.PubDate
			item.Size = p.Size
			item.Enclosure = p.Enclosure
			return nil
		}
	}
}

// parseFeed parses a Torznab/Newznab RSS response body
func parseFeed(body []byte, indexerName string) ([]*models.IndexerSearchResult, error) {
	var feed rssRoot
	if err := xml.Unmarshal(body, &feed); err != nil {
		return nil, fmt.Errorf("XML unmarshal error: %w", err)
	}

	results := make([]*models.IndexerSearchResult, 0, len(feed.Channel.Items))

	for _, item := range feed.Channel.Items {
		result := &models.IndexerSearchResult{
			Title:       item.Title,
			GUID:        item.GUID,
			Link:        item.Link,
			Size:        item.Size,
			Indexer:     indexerName,
			DownloadURL: item.Enclosure.URL,
		}

		// Parse pubDate with fallback
		if item.PubDate != "" {
			if t, err := time.Parse(time.RFC1123Z, item.PubDate); err == nil {
				result.PublishedAt = t
			} else if t, err := time.Parse(time.RFC1123, item.PubDate); err == nil {
				result.PublishedAt = t
			}
		}

		// Map named attrs
		for _, attr := range item.Attrs {
			switch strings.ToLower(attr.Name) {
			case "seeders":
				result.Seeders, _ = strconv.Atoi(attr.Value)
			case "leechers":
				result.Leechers, _ = strconv.Atoi(attr.Value)
			case "infohash":
				result.InfoHash = attr.Value
			case "magneturl":
				result.MagnetURL = attr.Value
			case "imdbid":
				result.ImdbID = attr.Value
			case "tvdbid":
				result.TvdbID = attr.Value
			case "season":
				result.Season, _ = strconv.Atoi(attr.Value)
			case "episode":
				result.Episode, _ = strconv.Atoi(attr.Value)
			case "category":
				if attr.Value != "" {
					result.Category = append(result.Category, attr.Value)
				}
			}
		}

		// If the enclosure didn't give us a download URL, fall back to Link
		if result.DownloadURL == "" {
			result.DownloadURL = item.Link
		}

		// Use enclosure length as size if the <size> element was absent
		if result.Size == 0 && item.Enclosure.Length > 0 {
			result.Size = item.Enclosure.Length
		}

		results = append(results, result)
	}

	return results, nil
}

// MultiIndexerSearch runs Search across multiple indexers concurrently and
// aggregates results, sorted by Seeders descending.
func MultiIndexerSearch(indexers []*models.Indexer, req *models.IndexerSearchRequest) ([]*models.IndexerSearchResult, []error) {
	type searchResult struct {
		results []*models.IndexerSearchResult
		err     error
	}

	ch := make(chan searchResult, len(indexers))

	var wg sync.WaitGroup
	for _, idx := range indexers {
		if !idx.Enabled {
			continue
		}
		wg.Add(1)
		go func(indexer *models.Indexer) {
			defer wg.Done()
			client := NewIndexerClient(indexer)
			results, err := client.Search(req)
			ch <- searchResult{results: results, err: err}
		}(idx)
	}

	// Close the channel once all goroutines finish.
	go func() {
		wg.Wait()
		close(ch)
	}()

	var (
		mu   sync.Mutex
		all  []*models.IndexerSearchResult
		errs []error
	)

	for sr := range ch {
		mu.Lock()
		if sr.err != nil {
			errs = append(errs, sr.err)
		} else {
			all = append(all, sr.results...)
		}
		mu.Unlock()
	}

	// Sort by Seeders descending
	sort.Slice(all, func(i, j int) bool {
		return all[i].Seeders > all[j].Seeders
	})

	return all, errs
}
