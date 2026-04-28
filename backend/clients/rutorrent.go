package clients

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"mime/multipart"
	"net/http"
	"net/url"
)

// RuTorrentClient is a client for the ruTorrent API
type RuTorrentClient struct {
	BaseURL    string
	Username   string
	Password   string
	HTTPClient *http.Client
}

// NewRuTorrentClient creates a new ruTorrent API client
func NewRuTorrentClient(baseURL, username, password string) *RuTorrentClient {
	return &RuTorrentClient{
		BaseURL:    baseURL,
		Username:   username,
		Password:   password,
		HTTPClient: NewHTTPClient(),
	}
}

// AddTorrentFile uploads a torrent file to ruTorrent
func (c *RuTorrentClient) AddTorrentFile(torrentData []byte, filename string, startNow bool) error {
	// Create the request URL
	reqURL := fmt.Sprintf("%s/php/addtorrent.php", c.BaseURL)

	// Create a buffer to write the multipart form data
	var b bytes.Buffer
	w := multipart.NewWriter(&b)

	// Add torrent file part
	part, err := w.CreateFormFile("torrent_file", filename)
	if err != nil {
		return fmt.Errorf("failed to create form file: %w", err)
	}
	if _, err := io.Copy(part, bytes.NewReader(torrentData)); err != nil {
		return fmt.Errorf("failed to copy torrent data: %w", err)
	}

	// Add other parameters
	if err := w.WriteField("torrents_start_stopped", fmt.Sprintf("%t", !startNow)); err != nil {
		return fmt.Errorf("failed to write field torrents_start_stopped: %w", err)
	}

	// Close the writer
	if err := w.Close(); err != nil {
		return fmt.Errorf("failed to close multipart writer: %w", err)
	}

	// Create HTTP request
	req, err := http.NewRequest("POST", reqURL, &b)
	if err != nil {
		return fmt.Errorf("failed to create request: %w", err)
	}

	// Set basic authentication if credentials are provided
	if c.Username != "" && c.Password != "" {
		auth := base64.StdEncoding.EncodeToString([]byte(c.Username + ":" + c.Password))
		req.Header.Set("Authorization", "Basic "+auth)
	}

	// Set content type with boundary
	req.Header.Set("Content-Type", w.FormDataContentType())

	// Send request
	resp, err := c.HTTPClient.Do(req)
	if err != nil {
		return fmt.Errorf("failed to send request: %w", err)
	}
	defer resp.Body.Close()

	// Check response status
	if resp.StatusCode != http.StatusOK {
		bodyBytes, _ := io.ReadAll(resp.Body)
		return fmt.Errorf("failed to add torrent: status code %d, response: %s", resp.StatusCode, string(bodyBytes))
	}

	return nil
}

// AddTorrentURL adds a torrent from a URL to ruTorrent
func (c *RuTorrentClient) AddTorrentURL(torrentURL string, startNow bool) error {
	// Create the request URL
	reqURL := fmt.Sprintf("%s/php/addtorrent.php", c.BaseURL)

	// Create form data
	form := url.Values{}
	form.Add("url", torrentURL)
	form.Add("torrents_start_stopped", fmt.Sprintf("%t", !startNow))

	// Create HTTP request
	req, err := http.NewRequest("POST", reqURL, bytes.NewBufferString(form.Encode()))
	if err != nil {
		return fmt.Errorf("failed to create request: %w", err)
	}

	// Set basic authentication if credentials are provided
	if c.Username != "" && c.Password != "" {
		auth := base64.StdEncoding.EncodeToString([]byte(c.Username + ":" + c.Password))
		req.Header.Set("Authorization", "Basic "+auth)
	}

	// Set content type
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")

	// Send request
	resp, err := c.HTTPClient.Do(req)
	if err != nil {
		return fmt.Errorf("failed to send request: %w", err)
	}
	defer resp.Body.Close()

	// Check response status
	if resp.StatusCode != http.StatusOK {
		bodyBytes, _ := io.ReadAll(resp.Body)
		return fmt.Errorf("failed to add torrent: status code %d, response: %s", resp.StatusCode, string(bodyBytes))
	}

	return nil
}

// GetTorrents retrieves the list of torrents from ruTorrent
func (c *RuTorrentClient) GetTorrents() (map[string]TorrentInfo, error) {
	// Create the request URL for the ruTorrent RPC plugin
	reqURL := fmt.Sprintf("%s/plugins/rpc/rpc.php", c.BaseURL)

	// Create HTTP request
	req, err := http.NewRequest("GET", reqURL, nil)
	if err != nil {
		return nil, fmt.Errorf("failed to create request: %w", err)
	}

	// Set basic authentication if credentials are provided
	if c.Username != "" && c.Password != "" {
		auth := base64.StdEncoding.EncodeToString([]byte(c.Username + ":" + c.Password))
		req.Header.Set("Authorization", "Basic "+auth)
	}

	// Send request
	resp, err := c.HTTPClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("failed to send request: %w", err)
	}
	defer resp.Body.Close()

	// Check response status
	if resp.StatusCode != http.StatusOK {
		bodyBytes, _ := io.ReadAll(resp.Body)
		return nil, fmt.Errorf("failed to get torrents: status code %d, response: %s", resp.StatusCode, string(bodyBytes))
	}

	// Parse response
	var result map[string]TorrentInfo
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, fmt.Errorf("failed to parse response: %w", err)
	}

	return result, nil
}

// RemoveTorrent removes a torrent from ruTorrent
func (c *RuTorrentClient) RemoveTorrent(hash string, removeData bool) error {
	// Create the request URL
	reqURL := fmt.Sprintf("%s/plugins/remove/action.php", c.BaseURL)

	// Create form data
	form := url.Values{}
	form.Add("hash", hash)
	if removeData {
		form.Add("remove_data", "1")
	}

	// Create HTTP request
	req, err := http.NewRequest("POST", reqURL, bytes.NewBufferString(form.Encode()))
	if err != nil {
		return fmt.Errorf("failed to create request: %w", err)
	}

	// Set basic authentication if credentials are provided
	if c.Username != "" && c.Password != "" {
		auth := base64.StdEncoding.EncodeToString([]byte(c.Username + ":" + c.Password))
		req.Header.Set("Authorization", "Basic "+auth)
	}

	// Set content type
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")

	// Send request
	resp, err := c.HTTPClient.Do(req)
	if err != nil {
		return fmt.Errorf("failed to send request: %w", err)
	}
	defer resp.Body.Close()

	// Check response status
	if resp.StatusCode != http.StatusOK {
		bodyBytes, _ := io.ReadAll(resp.Body)
		return fmt.Errorf("failed to remove torrent: status code %d, response: %s", resp.StatusCode, string(bodyBytes))
	}

	return nil
}

// StartTorrent starts a stopped torrent
func (c *RuTorrentClient) StartTorrent(hash string) error {
	return c.controlTorrent(hash, "start")
}

// StopTorrent stops a running torrent
func (c *RuTorrentClient) StopTorrent(hash string) error {
	return c.controlTorrent(hash, "stop")
}

// PauseTorrent pauses a running torrent
func (c *RuTorrentClient) PauseTorrent(hash string) error {
	return c.controlTorrent(hash, "pause")
}

// controlTorrent performs an action on a torrent
func (c *RuTorrentClient) controlTorrent(hash, action string) error {
	// Create the request URL
	reqURL := fmt.Sprintf("%s/php/", c.BaseURL)
	switch action {
	case "start":
		reqURL += "startbase.php"
	case "stop":
		reqURL += "stopbase.php"
	case "pause":
		reqURL += "pausebase.php"
	default:
		return fmt.Errorf("invalid action: %s", action)
	}

	// Create form data
	form := url.Values{}
	form.Add("hash", hash)

	// Create HTTP request
	req, err := http.NewRequest("POST", reqURL, bytes.NewBufferString(form.Encode()))
	if err != nil {
		return fmt.Errorf("failed to create request: %w", err)
	}

	// Set basic authentication if credentials are provided
	if c.Username != "" && c.Password != "" {
		auth := base64.StdEncoding.EncodeToString([]byte(c.Username + ":" + c.Password))
		req.Header.Set("Authorization", "Basic "+auth)
	}

	// Set content type
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")

	// Send request
	resp, err := c.HTTPClient.Do(req)
	if err != nil {
		return fmt.Errorf("failed to send request: %w", err)
	}
	defer resp.Body.Close()

	// Check response status
	if resp.StatusCode != http.StatusOK {
		bodyBytes, _ := io.ReadAll(resp.Body)
		return fmt.Errorf("failed to %s torrent: status code %d, response: %s", action, resp.StatusCode, string(bodyBytes))
	}

	return nil
}

// GetTorrentDetails gets detailed information about a specific torrent
func (c *RuTorrentClient) GetTorrentDetails(hash string) (*TorrentInfo, error) {
	torrents, err := c.GetTorrents()
	if err != nil {
		return nil, err
	}

	if info, ok := torrents[hash]; ok {
		return &info, nil
	}

	return nil, fmt.Errorf("torrent with hash %s not found", hash)
}

// CheckHealth verifies the connection to ruTorrent is working
func (c *RuTorrentClient) CheckHealth() (bool, string) {
	// Create the request URL for the ruTorrent RPC plugin
	reqURL := fmt.Sprintf("%s/plugins/rpc/rpc.php", c.BaseURL)

	// Create HTTP request
	req, err := http.NewRequest("GET", reqURL, nil)
	if err != nil {
		return false, fmt.Sprintf("Failed to create request: %s", err.Error())
	}

	// Set basic authentication if credentials are provided
	if c.Username != "" && c.Password != "" {
		auth := base64.StdEncoding.EncodeToString([]byte(c.Username + ":" + c.Password))
		req.Header.Set("Authorization", "Basic "+auth)
	}

	// Send request
	resp, err := c.HTTPClient.Do(req)
	if err != nil {
		return false, fmt.Sprintf("Failed to connect: %s", err.Error())
	}
	defer resp.Body.Close()

	// Check response status
	if resp.StatusCode != http.StatusOK {
		bodyBytes, _ := io.ReadAll(resp.Body)
		return false, fmt.Sprintf("Unexpected status code: %d, response: %s", resp.StatusCode, string(bodyBytes))
	}

	return true, "Connected successfully"
}

// GetClientInfo returns information about the ruTorrent client
func (c *RuTorrentClient) GetClientInfo() (*TorrentClientInfo, error) {
	// Get torrents to check connection and count
	torrents, err := c.GetTorrents()
	if err != nil {
		return nil, fmt.Errorf("failed to get client info: %w", err)
	}

	// Calculate global speeds
	var totalDownSpeed, totalUpSpeed int64
	for _, t := range torrents {
		totalDownSpeed += t.DownSpeed
		totalUpSpeed += t.UploadSpeed
	}

	// Create client info
	info := &TorrentClientInfo{
		ClientType:      "ruTorrent",
		Version:         "Unknown", // ruTorrent doesn't provide an easy way to get version
		ConnectedStatus: true,
		Message:         "Connected to ruTorrent",
		DownloadSpeed:   totalDownSpeed,
		UploadSpeed:     totalUpSpeed,
		TorrentsCount:   len(torrents),
	}

	return info, nil
}

// GetTorrentFiles returns the list of files for a specific torrent
func (c *RuTorrentClient) GetTorrentFiles(hash string) ([]string, error) {
	// Not directly supported by ruTorrent, would need to use XML-RPC and get_files action
	// This is complex, so for now we return an empty list
	return []string{}, nil
}
