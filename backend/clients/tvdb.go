package clients

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"time"

	"github.com/rs/zerolog/log"
)

// TVDBClient is a client for the TVDB API
type TVDBClient struct {
	BaseURL     string
	APIKey      string
	AccessToken string
	TokenExpiry time.Time
	HTTPClient  *http.Client
}

// TVDBAuthResponse represents the authentication response from TVDB
type TVDBAuthResponse struct {
	Token   string `json:"token"`
	Success bool   `json:"success"`
}

// TVDBSearchResponse represents the search response from TVDB
type TVDBSearchResponse struct {
	Data    []map[string]interface{} `json:"data"`
	Status  string                   `json:"status"`
	Success bool                     `json:"success"`
}

// TVDBPopularResponse represents the popular shows response from TVDB
type TVDBPopularResponse struct {
	Data    []map[string]interface{} `json:"data"`
	Status  string                   `json:"status"`
	Success bool                     `json:"success"`
}

// NewTVDBClient creates a new TVDB API client
func NewTVDBClient(apiKey string) *TVDBClient {
	return &TVDBClient{
		BaseURL:    "https://api.thetvdb.com",
		APIKey:     apiKey,
		HTTPClient: &http.Client{Timeout: 10 * time.Second},
	}
}

// Authenticate authenticates with the TVDB API
func (c *TVDBClient) Authenticate() error {
	// Check if we already have a valid token
	if c.AccessToken != "" && time.Now().Before(c.TokenExpiry) {
		return nil
	}

	// Prepare auth request
	authData := map[string]string{
		"apikey": c.APIKey,
	}
	jsonData, err := json.Marshal(authData)
	if err != nil {
		return err
	}

	// Create request
	req, err := http.NewRequest("POST", fmt.Sprintf("%s/login", c.BaseURL), bytes.NewBuffer(jsonData))
	if err != nil {
		return err
	}
	req.Header.Set("Content-Type", "application/json")

	// Send request
	resp, err := c.HTTPClient.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()

	// Check response status
	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("authentication failed with status code: %d", resp.StatusCode)
	}

	// Parse response
	var authResp TVDBAuthResponse
	if err := json.NewDecoder(resp.Body).Decode(&authResp); err != nil {
		return err
	}

	if !authResp.Success {
		return errors.New("authentication failed")
	}

	// Store token and expiry (tokens typically last 24 hours)
	c.AccessToken = authResp.Token
	c.TokenExpiry = time.Now().Add(23 * time.Hour)

	return nil
}

// makeAuthenticatedRequest makes a request with authentication to the TVDB API
func (c *TVDBClient) makeAuthenticatedRequest(method, endpoint string, body io.Reader) (*http.Response, error) {
	// Ensure we're authenticated
	if err := c.Authenticate(); err != nil {
		return nil, err
	}

	// Create request
	req, err := http.NewRequest(method, fmt.Sprintf("%s%s", c.BaseURL, endpoint), body)
	if err != nil {
		return nil, err
	}

	// Set headers
	req.Header.Set("Authorization", fmt.Sprintf("Bearer %s", c.AccessToken))
	req.Header.Set("Content-Type", "application/json")

	// Send request
	resp, err := c.HTTPClient.Do(req)
	if err != nil {
		return nil, err
	}
	// If the server returns 401, our token has expired — clear it so
	// the next call to Authenticate() forces a re-login.
	if resp.StatusCode == http.StatusUnauthorized {
		c.AccessToken = ""
		c.TokenExpiry = time.Time{}
		resp.Body.Close()
		return nil, fmt.Errorf("TVDB authentication expired (401); token cleared for retry")
	}
	return resp, nil
}

// SearchShows searches for TV shows by name
func (c *TVDBClient) SearchShows(name string) ([]map[string]interface{}, error) {
	// Make request
	resp, err := c.makeAuthenticatedRequest("GET", fmt.Sprintf("/search/series?name=%s", name), nil)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	// Check response status
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("search failed with status code: %d", resp.StatusCode)
	}

	// Parse response
	var searchResp TVDBSearchResponse
	if err := json.NewDecoder(resp.Body).Decode(&searchResp); err != nil {
		return nil, err
	}

	if !searchResp.Success {
		return nil, errors.New("search failed")
	}

	return searchResp.Data, nil
}

// GetPopularShows gets a list of popular TV shows
func (c *TVDBClient) GetPopularShows() ([]map[string]interface{}, error) {
	// Make request
	resp, err := c.makeAuthenticatedRequest("GET", "/popular", nil)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	// Check response status
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("get popular shows failed with status code: %d", resp.StatusCode)
	}

	// Parse response
	var popularResp TVDBPopularResponse
	if err := json.NewDecoder(resp.Body).Decode(&popularResp); err != nil {
		return nil, err
	}

	if !popularResp.Success {
		return nil, errors.New("get popular shows failed")
	}

	return popularResp.Data, nil
}

// GetEpisodes fetches all episodes for a series from TVDB.
// TVDB v4 paginates episodes; this method follows pages until exhausted.
// Endpoint: GET /series/:id/episodes/default?page=0
func (c *TVDBClient) GetEpisodes(seriesID string) ([]map[string]interface{}, error) {
	const maxPages = 50
	all := make([]map[string]interface{}, 0)

	for page := 0; page < maxPages; page++ {
		endpoint := fmt.Sprintf("/series/%s/episodes/default?page=%d", seriesID, page)
		resp, err := c.makeAuthenticatedRequest("GET", endpoint, nil)
		if err != nil {
			return nil, fmt.Errorf("GetEpisodes page %d: %w", page, err)
		}

		body, err := io.ReadAll(resp.Body)
		resp.Body.Close()
		if err != nil {
			return nil, fmt.Errorf("GetEpisodes page %d read body: %w", page, err)
		}

		if resp.StatusCode != http.StatusOK {
			return nil, fmt.Errorf("GetEpisodes page %d: unexpected status %d", page, resp.StatusCode)
		}

		var result struct {
			Data struct {
				Episodes []map[string]interface{} `json:"episodes"`
			} `json:"data"`
			Links struct {
				Next interface{} `json:"next"`
				Prev interface{} `json:"prev"`
			} `json:"links"`
		}
		if err := json.Unmarshal(body, &result); err != nil {
			return nil, fmt.Errorf("GetEpisodes page %d decode: %w", page, err)
		}

		eps := result.Data.Episodes
		log.Debug().Str("seriesID", seriesID).Int("page", page).Int("count", len(eps)).Msg("Fetched episode page")

		all = append(all, eps...)

		// Stop if no episodes returned or no next page.
		// TVDB returns "next": 0 (not null) when there is no next page;
		// float64(0) is what the JSON decoder produces for a zero number into interface{}.
		nextVal := result.Links.Next
		isNilNext := nextVal == nil || nextVal == float64(0)
		if len(eps) == 0 || isNilNext {
			break
		}
	}

	return all, nil
}

// GetShow gets details for a specific TV show by ID
func (c *TVDBClient) GetShow(id string) (map[string]interface{}, error) {
	// Make request
	resp, err := c.makeAuthenticatedRequest("GET", fmt.Sprintf("/series/%s", id), nil)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	// Check response status
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("get show failed with status code: %d", resp.StatusCode)
	}

	// Parse response
	var result struct {
		Data    map[string]interface{} `json:"data"`
		Success bool                   `json:"success"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, err
	}

	if !result.Success {
		return nil, errors.New("get show failed")
	}

	return result.Data, nil
}
