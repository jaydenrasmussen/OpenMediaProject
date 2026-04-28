package clients

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"time"
)

// DelugeClient is a client for the Deluge JSON-RPC API
type DelugeClient struct {
	BaseURL    string
	Password   string
	HTTPClient *http.Client
	SessionID  string
}

// DelugeRequest represents a request to the Deluge JSON-RPC API
type DelugeRequest struct {
	Method string        `json:"method"`
	Params []interface{} `json:"params"`
	ID     int           `json:"id"`
}

// DelugeResponse represents a response from the Deluge JSON-RPC API
type DelugeResponse struct {
	Result json.RawMessage `json:"result"`
	Error  *struct {
		Message string      `json:"message"`
		Code    int         `json:"code"`
		Data    interface{} `json:"data"`
	} `json:"error"`
	ID int `json:"id"`
}

// DelugeTorrent represents a torrent in Deluge
type DelugeTorrent struct {
	Hash           string          `json:"-"`
	Name           string          `json:"name"`
	State          string          `json:"state"`
	TotalSize      int64           `json:"total_size"`
	Progress       float64         `json:"progress"`
	DownloadRate   int64           `json:"download_payload_rate"`
	UploadRate     int64           `json:"upload_payload_rate"`
	Ratio          float64         `json:"ratio"`
	ETA            int             `json:"eta"`
	DownloadedEver int64           `json:"total_done"`
	UploadedEver   int64           `json:"total_uploaded"`
	TimeAdded      float64         `json:"time_added"`
	Trackers       []DelugeTracker `json:"trackers"`
	Files          []DelugeFile    `json:"files"`
	Error          string          `json:"message"`
	IsFinished     bool            `json:"is_finished"`
	IsActive       bool            `json:"-"`
}

// DelugeTracker represents a tracker in Deluge
type DelugeTracker struct {
	URL      string `json:"url"`
	Status   int    `json:"status"`
	Tier     int    `json:"tier"`
	Complete int    `json:"complete"`
}

// DelugeFile represents a file in Deluge
type DelugeFile struct {
	Path     string  `json:"path"`
	Size     int64   `json:"size"`
	Offset   int64   `json:"offset"`
	Index    int     `json:"index"`
	Progress float64 `json:"progress"`
}

// NewDelugeClient creates a new Deluge API client
func NewDelugeClient(baseURL, password string) *DelugeClient {
	return &DelugeClient{
		BaseURL:    baseURL,
		Password:   password,
		HTTPClient: NewHTTPClient(),
	}
}

// sendRequest sends a request to the Deluge JSON-RPC API
func (c *DelugeClient) sendRequest(method string, params ...interface{}) (*DelugeResponse, error) {
	// Check if we need to authenticate
	if c.SessionID == "" {
		if err := c.authenticate(); err != nil {
			return nil, err
		}
	}

	// Create request
	req := DelugeRequest{
		Method: method,
		Params: params,
		ID:     int(time.Now().Unix()),
	}

	// Convert request to JSON
	reqBody, err := json.Marshal(req)
	if err != nil {
		return nil, fmt.Errorf("failed to marshal request: %w", err)
	}

	// Create HTTP request
	httpReq, err := http.NewRequest("POST", c.BaseURL+"/json", bytes.NewBuffer(reqBody))
	if err != nil {
		return nil, fmt.Errorf("failed to create request: %w", err)
	}

	// Set headers
	httpReq.Header.Set("Content-Type", "application/json")
	httpReq.Header.Set("Accept", "application/json")

	// Add cookie if authenticated
	if c.SessionID != "" {
		httpReq.AddCookie(&http.Cookie{
			Name:  "_session_id",
			Value: c.SessionID,
		})
	}

	// Send request
	resp, err := c.HTTPClient.Do(httpReq)
	if err != nil {
		return nil, fmt.Errorf("failed to send request: %w", err)
	}
	defer resp.Body.Close()

	// Check response status
	if resp.StatusCode != http.StatusOK {
		bodyBytes, _ := io.ReadAll(resp.Body)
		return nil, fmt.Errorf("unexpected status code: %d, response: %s", resp.StatusCode, string(bodyBytes))
	}

	// Parse response
	var result DelugeResponse
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, fmt.Errorf("failed to parse response: %w", err)
	}

	// Check for RPC error
	if result.Error != nil {
		return nil, fmt.Errorf("RPC error: %s (code: %d)", result.Error.Message, result.Error.Code)
	}

	return &result, nil
}

// authenticate authenticates with the Deluge server
func (c *DelugeClient) authenticate() error {
	// Create request
	req := DelugeRequest{
		Method: "auth.login",
		Params: []interface{}{c.Password},
		ID:     int(time.Now().Unix()),
	}

	// Convert request to JSON
	reqBody, err := json.Marshal(req)
	if err != nil {
		return fmt.Errorf("failed to marshal auth request: %w", err)
	}

	// Create HTTP request
	httpReq, err := http.NewRequest("POST", c.BaseURL+"/json", bytes.NewBuffer(reqBody))
	if err != nil {
		return fmt.Errorf("failed to create auth request: %w", err)
	}

	// Set headers
	httpReq.Header.Set("Content-Type", "application/json")
	httpReq.Header.Set("Accept", "application/json")

	// Send request
	resp, err := c.HTTPClient.Do(httpReq)
	if err != nil {
		return fmt.Errorf("failed to send auth request: %w", err)
	}
	defer resp.Body.Close()

	// Check response status
	if resp.StatusCode != http.StatusOK {
		bodyBytes, _ := io.ReadAll(resp.Body)
		return fmt.Errorf("unexpected status code during auth: %d, response: %s", resp.StatusCode, string(bodyBytes))
	}

	// Parse response
	var result DelugeResponse
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return fmt.Errorf("failed to parse auth response: %w", err)
	}

	// Check for RPC error
	if result.Error != nil {
		return fmt.Errorf("auth error: %s (code: %d)", result.Error.Message, result.Error.Code)
	}

	// Check auth result
	var success bool
	if err := json.Unmarshal(result.Result, &success); err != nil {
		return fmt.Errorf("failed to parse auth result: %w", err)
	}

	if !success {
		return fmt.Errorf("authentication failed")
	}

	// Extract session ID from cookies
	for _, cookie := range resp.Cookies() {
		if cookie.Name == "_session_id" {
			c.SessionID = cookie.Value
			return nil
		}
	}

	return fmt.Errorf("no session ID in response")
}

// AddTorrentFile uploads a torrent file to Deluge
func (c *DelugeClient) AddTorrentFile(torrentData []byte, filename string, startNow bool) error {
	// Encode torrent data as base64
	fileData := base64.StdEncoding.EncodeToString(torrentData)

	// Options
	options := map[string]interface{}{
		"add_paused": !startNow,
	}

	// Send request
	resp, err := c.sendRequest("core.add_torrent_file", filename, fileData, options)
	if err != nil {
		return err
	}

	// Check result
	var torrentID string
	if err := json.Unmarshal(resp.Result, &torrentID); err != nil {
		// If result is null, it might mean the torrent was already added
		if string(resp.Result) == "null" {
			return nil
		}
		return fmt.Errorf("failed to parse add_torrent_file result: %w", err)
	}

	return nil
}

// AddTorrentURL adds a torrent from a URL to Deluge
func (c *DelugeClient) AddTorrentURL(torrentURL string, startNow bool) error {
	// Options
	options := map[string]interface{}{
		"add_paused": !startNow,
	}

	// Send request
	resp, err := c.sendRequest("core.add_torrent_url", torrentURL, options)
	if err != nil {
		return err
	}

	// Check result
	var torrentID string
	if err := json.Unmarshal(resp.Result, &torrentID); err != nil {
		// If result is null, it might mean the torrent was already added
		if string(resp.Result) == "null" {
			return nil
		}
		return fmt.Errorf("failed to parse add_torrent_url result: %w", err)
	}

	return nil
}

// GetTorrents retrieves the list of torrents from Deluge
func (c *DelugeClient) GetTorrents() (map[string]TorrentInfo, error) {
	// Send request to get torrent status fields
	resp, err := c.sendRequest("core.get_torrents_status", map[string]interface{}{}, []string{
		"name", "state", "total_size", "progress", "download_payload_rate", "upload_payload_rate",
		"ratio", "eta", "total_done", "total_uploaded", "time_added", "trackers", "files",
		"message", "is_finished", "active_time",
	})
	if err != nil {
		return nil, err
	}

	// Parse response
	var torrents map[string]DelugeTorrent
	if err := json.Unmarshal(resp.Result, &torrents); err != nil {
		return nil, fmt.Errorf("failed to parse get_torrents_status result: %w", err)
	}

	// Convert to common TorrentInfo format
	result := make(map[string]TorrentInfo)
	for hash, t := range torrents {
		// Add hash to the struct
		t.Hash = hash
		t.IsActive = t.State == "Downloading" || t.State == "Seeding"

		// Convert to TorrentInfo
		result[hash] = TorrentInfo{
			Hash:        hash,
			Name:        t.Name,
			Size:        t.TotalSize,
			Downloaded:  t.DownloadedEver,
			Uploaded:    t.UploadedEver,
			Ratio:       t.Ratio,
			UploadSpeed: t.UploadRate,
			DownSpeed:   t.DownloadRate,
			ETA:         t.ETA,
			State:       c.convertState(t.State),
			Status:      t.State,
			Message:     t.Error,
			Created:     int64(t.TimeAdded),
			Active:      t.IsActive,
		}
	}

	return result, nil
}

// RemoveTorrent removes a torrent from Deluge
func (c *DelugeClient) RemoveTorrent(hash string, removeData bool) error {
	// Send request
	_, err := c.sendRequest("core.remove_torrent", hash, removeData)
	return err
}

// StartTorrent starts a stopped torrent
func (c *DelugeClient) StartTorrent(hash string) error {
	// Send request
	_, err := c.sendRequest("core.resume_torrent", []string{hash})
	return err
}

// StopTorrent stops a running torrent
func (c *DelugeClient) StopTorrent(hash string) error {
	// Send request
	_, err := c.sendRequest("core.pause_torrent", []string{hash})
	return err
}

// PauseTorrent pauses a running torrent (same as stop in Deluge)
func (c *DelugeClient) PauseTorrent(hash string) error {
	return c.StopTorrent(hash)
}

// GetTorrentDetails gets detailed information about a specific torrent
func (c *DelugeClient) GetTorrentDetails(hash string) (*TorrentInfo, error) {
	// Send request to get torrent status fields
	resp, err := c.sendRequest("core.get_torrent_status", hash, []string{
		"name", "state", "total_size", "progress", "download_payload_rate", "upload_payload_rate",
		"ratio", "eta", "total_done", "total_uploaded", "time_added", "trackers", "files",
		"message", "is_finished", "active_time",
	})
	if err != nil {
		return nil, err
	}

	// Parse response
	var t DelugeTorrent
	if err := json.Unmarshal(resp.Result, &t); err != nil {
		return nil, fmt.Errorf("failed to parse get_torrent_status result: %w", err)
	}

	// Check if torrent was found
	if t.Name == "" {
		return nil, fmt.Errorf("torrent with hash %s not found", hash)
	}

	// Add hash to the struct
	t.Hash = hash
	t.IsActive = t.State == "Downloading" || t.State == "Seeding"

	// Convert to TorrentInfo
	info := &TorrentInfo{
		Hash:        hash,
		Name:        t.Name,
		Size:        t.TotalSize,
		Downloaded:  t.DownloadedEver,
		Uploaded:    t.UploadedEver,
		Ratio:       t.Ratio,
		UploadSpeed: t.UploadRate,
		DownSpeed:   t.DownloadRate,
		ETA:         t.ETA,
		State:       c.convertState(t.State),
		Status:      t.State,
		Message:     t.Error,
		Created:     int64(t.TimeAdded),
		Active:      t.IsActive,
	}

	return info, nil
}

// convertState converts Deluge state to a common state string
func (c *DelugeClient) convertState(state string) string {
	switch state {
	case "Paused":
		return "stopped"
	case "Checking":
		return "checking"
	case "Downloading":
		return "downloading"
	case "Seeding":
		return "seeding"
	case "Error":
		return "error"
	case "Queued":
		return "queued"
	default:
		return "unknown_" + state
	}
}

// CheckHealth verifies the connection to Deluge is working
func (c *DelugeClient) CheckHealth() (bool, string) {
	// Try to authenticate to check connectivity
	if c.SessionID == "" {
		if err := c.authenticate(); err != nil {
			return false, fmt.Sprintf("Failed to connect: %s", err.Error())
		}
	}

	// Now try a simple API call to verify everything works
	resp, err := c.sendRequest("web.connected")
	if err != nil {
		return false, fmt.Sprintf("Failed to verify connection: %s", err.Error())
	}

	// Parse the response
	var connected bool
	if err := json.Unmarshal(resp.Result, &connected); err != nil {
		return false, fmt.Sprintf("Failed to parse response: %s", err.Error())
	}

	if !connected {
		return false, "Deluge WebUI is not connected to a daemon"
	}

	return true, "Connected successfully"
}

// GetClientInfo returns information about the Deluge client
func (c *DelugeClient) GetClientInfo() (*TorrentClientInfo, error) {
	// Get client version
	resp, err := c.sendRequest("web.get_config")
	if err != nil {
		return nil, fmt.Errorf("failed to get client info: %w", err)
	}

	// Parse config to get version
	var config map[string]interface{}
	if err := json.Unmarshal(resp.Result, &config); err != nil {
		return nil, fmt.Errorf("failed to parse config: %w", err)
	}

	// Get daemon info
	daemonResp, err := c.sendRequest("web.connected")
	if err != nil {
		return nil, fmt.Errorf("failed to check daemon connection: %w", err)
	}

	var connected bool
	if err := json.Unmarshal(daemonResp.Result, &connected); err != nil {
		return nil, fmt.Errorf("failed to parse daemon connection: %w", err)
	}

	// Get session status to retrieve global speeds
	sessionResp, err := c.sendRequest("core.get_session_status", []string{
		"download_rate", "upload_rate", "total_download", "total_upload",
	})
	if err != nil {
		return nil, fmt.Errorf("failed to get session status: %w", err)
	}

	// Parse session status
	var sessionStatus map[string]interface{}
	if err := json.Unmarshal(sessionResp.Result, &sessionStatus); err != nil {
		return nil, fmt.Errorf("failed to parse session status: %w", err)
	}

	// Get torrent count
	torrents, err := c.GetTorrents()
	if err != nil {
		return nil, fmt.Errorf("failed to get torrents: %w", err)
	}

	// Extract download and upload speeds
	downloadSpeed := int64(0)
	uploadSpeed := int64(0)
	if val, ok := sessionStatus["download_rate"]; ok {
		if f, ok := val.(float64); ok {
			downloadSpeed = int64(f)
		}
	}
	if val, ok := sessionStatus["upload_rate"]; ok {
		if f, ok := val.(float64); ok {
			uploadSpeed = int64(f)
		}
	}

	// Create client info
	info := &TorrentClientInfo{
		ClientType:      "Deluge",
		Version:         "Unknown", // Deluge web UI doesn't easily expose version
		ConnectedStatus: connected,
		Message:         "Connected to Deluge",
		DownloadSpeed:   downloadSpeed,
		UploadSpeed:     uploadSpeed,
		TorrentsCount:   len(torrents),
	}

	return info, nil
}

// GetTorrentFiles returns the list of files for a specific torrent
func (c *DelugeClient) GetTorrentFiles(hash string) ([]string, error) {
	// Send request to get torrent status fields
	resp, err := c.sendRequest("core.get_torrent_status", hash, []string{"files"})
	if err != nil {
		return nil, err
	}

	// Parse response
	var t DelugeTorrent
	if err := json.Unmarshal(resp.Result, &t); err != nil {
		return nil, fmt.Errorf("failed to parse get_torrent_status result: %w", err)
	}

	// Check if torrent was found
	if t.Name == "" {
		return nil, fmt.Errorf("torrent with hash %s not found", hash)
	}

	// Extract file paths
	var files []string
	for _, f := range t.Files {
		files = append(files, f.Path)
	}

	return files, nil
}
