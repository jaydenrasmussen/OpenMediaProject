package clients

import (
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
	"time"
)

// OMDBClient fetches external ratings (IMDB, Rotten Tomatoes, Metacritic)
// from the Open Movie Database API (omdbapi.com).
type OMDBClient struct {
	APIKey     string
	HTTPClient *http.Client
}

// OMDBRatings holds the rating data returned by OMDB for a title.
type OMDBRatings struct {
	IMDBRating     string // e.g. "9.5"
	RottenTomatoes string // e.g. "96%"
	Metacritic     string // e.g. "99"
}

// NewOMDBClient creates a new OMDB client.
func NewOMDBClient(apiKey string) *OMDBClient {
	return &OMDBClient{
		APIKey:     apiKey,
		HTTPClient: &http.Client{Timeout: 8 * time.Second},
	}
}

// GetRatings fetches ratings for a title identified by its IMDB ID.
// Returns nil if the title is not found or OMDB returns an error response.
func (c *OMDBClient) GetRatings(imdbID string) (*OMDBRatings, error) {
	u := fmt.Sprintf(
		"https://www.omdbapi.com/?apikey=%s&i=%s",
		url.QueryEscape(c.APIKey),
		url.QueryEscape(imdbID),
	)
	resp, err := c.HTTPClient.Get(u)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	var raw struct {
		Response   string `json:"Response"`
		ImdbRating string `json:"imdbRating"`
		Metascore  string `json:"Metascore"`
		Ratings    []struct {
			Source string `json:"Source"`
			Value  string `json:"Value"`
		} `json:"Ratings"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&raw); err != nil {
		return nil, err
	}
	if raw.Response != "True" {
		return nil, fmt.Errorf("OMDB: title not found for %s", imdbID)
	}

	out := &OMDBRatings{
		IMDBRating: raw.ImdbRating,
		Metacritic: raw.Metascore,
	}
	for _, r := range raw.Ratings {
		if r.Source == "Rotten Tomatoes" {
			out.RottenTomatoes = r.Value
		}
	}
	return out, nil
}
