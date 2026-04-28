package clients

import (
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
	"time"
)

// TMDBClient is a client for The Movie Database (TMDB) API
type TMDBClient struct {
	BaseURL    string
	APIKey     string
	HTTPClient *http.Client
}

// TMDBSearchResponse represents the search response from TMDB
type TMDBSearchResponse struct {
	Page         int                      `json:"page"`
	Results      []map[string]interface{} `json:"results"`
	TotalPages   int                      `json:"total_pages"`
	TotalResults int                      `json:"total_results"`
}

// TMDBPopularResponse represents the popular movies response from TMDB
type TMDBPopularResponse struct {
	Page         int                      `json:"page"`
	Results      []map[string]interface{} `json:"results"`
	TotalPages   int                      `json:"total_pages"`
	TotalResults int                      `json:"total_results"`
}

// TMDBMovieResponse represents a movie response from TMDB
type TMDBMovieResponse struct {
	Data map[string]interface{} `json:"data"`
}

// NewTMDBClient creates a new TMDB API client
func NewTMDBClient(apiKey string) *TMDBClient {
	return &TMDBClient{
		BaseURL:    "https://api.themoviedb.org/3",
		APIKey:     apiKey,
		HTTPClient: &http.Client{Timeout: 10 * time.Second},
	}
}

// makeRequest makes a request to the TMDB API
func (c *TMDBClient) makeRequest(method, endpoint string, params map[string]string) (*http.Response, error) {
	// Create request URL with query parameters
	reqURL, err := url.Parse(fmt.Sprintf("%s%s", c.BaseURL, endpoint))
	if err != nil {
		return nil, err
	}

	// Add query parameters
	query := reqURL.Query()
	query.Set("api_key", c.APIKey)

	// Add any additional parameters
	for key, value := range params {
		query.Set(key, value)
	}

	reqURL.RawQuery = query.Encode()

	// Create request
	req, err := http.NewRequest(method, reqURL.String(), nil)
	if err != nil {
		return nil, err
	}

	// Set headers
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Accept", "application/json")

	// Send request
	return c.HTTPClient.Do(req)
}

// SearchMovies searches for movies by title
func (c *TMDBClient) SearchMovies(query string, page int) (*TMDBSearchResponse, error) {
	// Create parameters
	params := map[string]string{
		"query": query,
		"page":  fmt.Sprintf("%d", page),
	}

	// Make request
	resp, err := c.makeRequest("GET", "/search/movie", params)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	// Check response status
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("search failed with status code: %d", resp.StatusCode)
	}

	// Parse response
	var searchResp TMDBSearchResponse
	if err := json.NewDecoder(resp.Body).Decode(&searchResp); err != nil {
		return nil, err
	}

	return &searchResp, nil
}

// GetPopularMovies gets a list of popular movies
func (c *TMDBClient) GetPopularMovies(page int) (*TMDBPopularResponse, error) {
	// Create parameters
	params := map[string]string{
		"page": fmt.Sprintf("%d", page),
	}

	// Make request
	resp, err := c.makeRequest("GET", "/movie/popular", params)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	// Check response status
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("get popular movies failed with status code: %d", resp.StatusCode)
	}

	// Parse response
	var popularResp TMDBPopularResponse
	if err := json.NewDecoder(resp.Body).Decode(&popularResp); err != nil {
		return nil, err
	}

	return &popularResp, nil
}

// GetMovie gets details for a specific movie by ID
func (c *TMDBClient) GetMovie(id string) (map[string]interface{}, error) {
	// Create parameters with append_to_response to get additional details
	params := map[string]string{
		"append_to_response": "credits,images,videos",
	}

	// Make request
	resp, err := c.makeRequest("GET", fmt.Sprintf("/movie/%s", id), params)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	// Check response status
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("get movie failed with status code: %d", resp.StatusCode)
	}

	// Parse response
	var result map[string]interface{}
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, err
	}

	return result, nil
}

// GetNowPlayingMovies gets a list of movies currently in theaters
func (c *TMDBClient) GetNowPlayingMovies(page int) (*TMDBPopularResponse, error) {
	// Create parameters
	params := map[string]string{
		"page": fmt.Sprintf("%d", page),
	}

	// Make request
	resp, err := c.makeRequest("GET", "/movie/now_playing", params)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	// Check response status
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("get now playing movies failed with status code: %d", resp.StatusCode)
	}

	// Parse response
	var nowPlayingResp TMDBPopularResponse
	if err := json.NewDecoder(resp.Body).Decode(&nowPlayingResp); err != nil {
		return nil, err
	}

	return &nowPlayingResp, nil
}

// GetTopRatedMovies gets a list of top rated movies
func (c *TMDBClient) GetTopRatedMovies(page int) (*TMDBPopularResponse, error) {
	// Create parameters
	params := map[string]string{
		"page": fmt.Sprintf("%d", page),
	}

	// Make request
	resp, err := c.makeRequest("GET", "/movie/top_rated", params)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	// Check response status
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("get top rated movies failed with status code: %d", resp.StatusCode)
	}

	// Parse response
	var topRatedResp TMDBPopularResponse
	if err := json.NewDecoder(resp.Body).Decode(&topRatedResp); err != nil {
		return nil, err
	}

	return &topRatedResp, nil
}

// GetUpcomingMovies gets a list of upcoming movies
func (c *TMDBClient) GetUpcomingMovies(page int) (*TMDBPopularResponse, error) {
	// Create parameters
	params := map[string]string{
		"page": fmt.Sprintf("%d", page),
	}

	// Make request
	resp, err := c.makeRequest("GET", "/movie/upcoming", params)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	// Check response status
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("get upcoming movies failed with status code: %d", resp.StatusCode)
	}

	// Parse response
	var upcomingResp TMDBPopularResponse
	if err := json.NewDecoder(resp.Body).Decode(&upcomingResp); err != nil {
		return nil, err
	}

	return &upcomingResp, nil
}
