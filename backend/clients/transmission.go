package clients

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strconv"
	"time"
)

// TransmissionClient is a client for the Transmission API
type TransmissionClient struct {
	BaseURL     string
	Username    string
	Password    string
	HTTPClient  *http.Client
	SessionID   string
	SessionTime time.Time
}

// Transmission RPC constants
const (
	transmissionRPCEndpoint = "/transmission/rpc"
	sessionIDHeader         = "X-Transmission-Session-Id"
	sessionIDExpiry         = 15 * time.Minute
)

// Transmission RPC method names
const (
	methodTorrentAdd      = "torrent-add"
	methodTorrentRemove   = "torrent-remove"
	methodTorrentStart    = "torrent-start"
	methodTorrentStop     = "torrent-stop"
	methodTorrentGet      = "torrent-get"
	methodSessionStats    = "session-stats"
	methodSessionGet      = "session-get"
	methodPortTest        = "port-test"
	methodBlocklistUpdate = "blocklist-update"
	methodFreeSpace       = "free-space"
	methodSessionClose    = "session-close"
)

// TransmissionRequest represents a request to the Transmission RPC API
type TransmissionRequest struct {
	Method    string      `json:"method"`
	Arguments interface{} `json:"arguments"`
	Tag       int         `json:"tag,omitempty"`
}

// TransmissionResponse represents a response from the Transmission RPC API
type TransmissionResponse struct {
	Result    string          `json:"result"`
	Arguments json.RawMessage `json:"arguments"`
	Tag       int             `json:"tag,omitempty"`
}

// TorrentAddArguments represents arguments for the torrent-add method
type TorrentAddArguments struct {
	Filename          string `json:"filename,omitempty"`          // URL or magnet link
	MetaInfo          string `json:"metainfo,omitempty"`          // Base64-encoded .torrent file
	DownloadDir       string `json:"download-dir,omitempty"`      // Download directory
	Paused            bool   `json:"paused"`                      // Add torrent in paused state
	PeerLimit         int    `json:"peer-limit,omitempty"`        // Maximum number of peers
	BandwidthPriority int    `json:"bandwidthPriority,omitempty"` // Priority (-1, 0, 1)
	FilesWanted       []int  `json:"files-wanted,omitempty"`      // Indices of files to download
	FilesUnwanted     []int  `json:"files-unwanted,omitempty"`    // Indices of files to skip
}

// TorrentAddResponse represents the response from the torrent-add method
type TorrentAddResponse struct {
	TorrentAdded struct {
		ID         int    `json:"id"`
		Name       string `json:"name"`
		HashString string `json:"hashString"`
	} `json:"torrent-added"`
	TorrentDuplicate struct {
		ID         int    `json:"id"`
		Name       string `json:"name"`
		HashString string `json:"hashString"`
	} `json:"torrent-duplicate"`
}

// TorrentRemoveArguments represents arguments for the torrent-remove method
type TorrentRemoveArguments struct {
	IDs             []string `json:"ids"`               // Array of torrent IDs or hashes
	DeleteLocalData bool     `json:"delete-local-data"` // Whether to delete local data
}

// TorrentGetArguments represents arguments for the torrent-get method
type TorrentGetArguments struct {
	IDs    []string `json:"ids,omitempty"` // Array of torrent IDs or hashes
	Fields []string `json:"fields"`        // Array of field names to retrieve
}

// Torrent represents a torrent in Transmission
type Torrent struct {
	ID                 int     `json:"id"`
	Name               string  `json:"name"`
	HashString         string  `json:"hashString"`
	Status             int     `json:"status"`
	Error              int     `json:"error"`
	ErrorString        string  `json:"errorString"`
	DownloadDir        string  `json:"downloadDir"`
	PercentDone        float64 `json:"percentDone"`
	RateDownload       int64   `json:"rateDownload"`
	RateUpload         int64   `json:"rateUpload"`
	SizeWhenDone       int64   `json:"sizeWhenDone"`
	TotalSize          int64   `json:"totalSize"`
	UploadRatio        float64 `json:"uploadRatio"`
	Eta                int     `json:"eta"`
	DownloadedEver     int64   `json:"downloadedEver"`
	UploadedEver       int64   `json:"uploadedEver"`
	AddedDate          int64   `json:"addedDate"`
	TorrentFile        string  `json:"torrentFile"`
	PeersConnected     int     `json:"peersConnected"`
	PeersGettingFromUs int     `json:"peersGettingFromUs"`
	PeersSendingToUs   int     `json:"peersSendingToUs"`
	ActivityDate       int64   `json:"activityDate"`
	DateCreated        int64   `json:"dateCreated"`
	LeftUntilDone      int64   `json:"leftUntilDone"`
	IsFinished         bool    `json:"isFinished"`
	IsStalled          bool    `json:"isStalled"`
}

// TorrentGetResponse represents the response from the torrent-get method
type TorrentGetResponse struct {
	Torrents []Torrent `json:"torrents"`
}

// Transmission status constants
const (
	StatusStopped      = 0 // Torrent is stopped
	StatusCheckWait    = 1 // Torrent is queued for checking
	StatusCheck        = 2 // Torrent is checking files
	StatusDownloadWait = 3 // Torrent is queued for download
	StatusDownload     = 4 // Torrent is downloading
	StatusSeedWait     = 5 // Torrent is queued for seeding
	StatusSeed         = 6 // Torrent is seeding
)

// NewTransmissionClient creates a new Transmission API client
func NewTransmissionClient(baseURL, username, password string) *TransmissionClient {
	return &TransmissionClient{
		BaseURL:    baseURL,
		Username:   username,
		Password:   password,
		HTTPClient: NewHTTPClient(),
	}
}

// sendRequest sends a request to the Transmission RPC API
func (c *TransmissionClient) sendRequest(method string, args interface{}) (*TransmissionResponse, error) {
	// Check if we need to renew the session ID
	if c.SessionID == "" || time.Since(c.SessionTime) > sessionIDExpiry {
		if err := c.getSessionID(); err != nil {
			return nil, err
		}
	}

	// Create request
	req := TransmissionRequest{
		Method:    method,
		Arguments: args,
	}

	// Convert request to JSON
	reqBody, err := json.Marshal(req)
	if err != nil {
		return nil, fmt.Errorf("failed to marshal request: %w", err)
	}

	// Create HTTP request
	url := c.BaseURL + transmissionRPCEndpoint
	httpReq, err := http.NewRequest("POST", url, bytes.NewBuffer(reqBody))
	if err != nil {
		return nil, fmt.Errorf("failed to create request: %w", err)
	}

	// Set headers
	httpReq.Header.Set("Content-Type", "application/json")
	httpReq.Header.Set(sessionIDHeader, c.SessionID)

	// Set basic authentication if credentials are provided
	if c.Username != "" && c.Password != "" {
		auth := base64.StdEncoding.EncodeToString([]byte(c.Username + ":" + c.Password))
		httpReq.Header.Set("Authorization", "Basic "+auth)
	}

	// Send request
	resp, err := c.HTTPClient.Do(httpReq)
	if err != nil {
		return nil, fmt.Errorf("failed to send request: %w", err)
	}
	defer resp.Body.Close()

	// Check if session ID has expired
	if resp.StatusCode == http.StatusConflict {
		// Extract new session ID and retry
		c.SessionID = resp.Header.Get(sessionIDHeader)
		c.SessionTime = time.Now()
		return c.sendRequest(method, args)
	}

	// Check response status
	if resp.StatusCode != http.StatusOK {
		bodyBytes, _ := io.ReadAll(resp.Body)
		return nil, fmt.Errorf("unexpected status code: %d, response: %s", resp.StatusCode, string(bodyBytes))
	}

	// Parse response
	var result TransmissionResponse
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, fmt.Errorf("failed to parse response: %w", err)
	}

	// Check for RPC error
	if result.Result != "success" {
		return nil, fmt.Errorf("RPC error: %s", result.Result)
	}

	return &result, nil
}

// getSessionID gets a new session ID from the Transmission server
func (c *TransmissionClient) getSessionID() error {
	// Create HTTP request
	url := c.BaseURL + transmissionRPCEndpoint
	req, err := http.NewRequest("POST", url, nil)
	if err != nil {
		return fmt.Errorf("failed to create request: %w", err)
	}

	// Set basic authentication if credentials are provided
	if c.Username != "" && c.Password != "" {
		auth := base64.StdEncoding.EncodeToString([]byte(c.Username + ":" + c.Password))
		req.Header.Set("Authorization", "Basic "+auth)
	}

	// Send request
	resp, err := c.HTTPClient.Do(req)
	if err != nil {
		return fmt.Errorf("failed to send request: %w", err)
	}
	defer resp.Body.Close()

	// We expect a 409 Conflict response with the session ID
	if resp.StatusCode != http.StatusConflict {
		bodyBytes, _ := io.ReadAll(resp.Body)
		return fmt.Errorf("unexpected status code: %d, response: %s", resp.StatusCode, string(bodyBytes))
	}

	// Extract session ID
	sessionID := resp.Header.Get(sessionIDHeader)
	if sessionID == "" {
		return fmt.Errorf("no session ID in response")
	}

	c.SessionID = sessionID
	c.SessionTime = time.Now()
	return nil
}

// AddTorrentFile uploads a torrent file to Transmission
func (c *TransmissionClient) AddTorrentFile(torrentData []byte, filename string, startNow bool) error {
	// Encode torrent data as base64
	metainfo := base64.StdEncoding.EncodeToString(torrentData)

	// Create arguments
	args := TorrentAddArguments{
		MetaInfo: metainfo,
		Paused:   !startNow,
	}

	// Send request
	resp, err := c.sendRequest(methodTorrentAdd, args)
	if err != nil {
		return err
	}

	// Parse response
	var addResp TorrentAddResponse
	if err := json.Unmarshal(resp.Arguments, &addResp); err != nil {
		return fmt.Errorf("failed to parse torrent-add response: %w", err)
	}

	return nil
}

// AddTorrentURL adds a torrent from a URL to Transmission
func (c *TransmissionClient) AddTorrentURL(torrentURL string, startNow bool) error {
	// Create arguments
	args := TorrentAddArguments{
		Filename: torrentURL,
		Paused:   !startNow,
	}

	// Send request
	resp, err := c.sendRequest(methodTorrentAdd, args)
	if err != nil {
		return err
	}

	// Parse response
	var addResp TorrentAddResponse
	if err := json.Unmarshal(resp.Arguments, &addResp); err != nil {
		return fmt.Errorf("failed to parse torrent-add response: %w", err)
	}

	return nil
}

// GetTorrents retrieves the list of torrents from Transmission
func (c *TransmissionClient) GetTorrents() (map[string]TorrentInfo, error) {
	// Define fields to retrieve
	fields := []string{
		"id", "name", "hashString", "status", "error", "errorString",
		"percentDone", "rateDownload", "rateUpload", "sizeWhenDone",
		"totalSize", "uploadRatio", "eta", "downloadedEver", "uploadedEver",
		"addedDate", "isFinished", "isStalled", "leftUntilDone",
	}

	// Create arguments
	args := TorrentGetArguments{
		Fields: fields,
	}

	// Send request
	resp, err := c.sendRequest(methodTorrentGet, args)
	if err != nil {
		return nil, err
	}

	// Parse response
	var getResp TorrentGetResponse
	if err := json.Unmarshal(resp.Arguments, &getResp); err != nil {
		return nil, fmt.Errorf("failed to parse torrent-get response: %w", err)
	}

	// Convert to common TorrentInfo format
	result := make(map[string]TorrentInfo)
	for _, t := range getResp.Torrents {
		// Convert status to a human-readable string
		status := c.getStatusString(t.Status)

		// Calculate progress (used in future feature implementations)
		_ = t.PercentDone * 100

		// Convert state
		state := c.getStateString(t.Status, t.Error, t.IsStalled)

		result[t.HashString] = TorrentInfo{
			Hash:        t.HashString,
			Name:        t.Name,
			Size:        t.TotalSize,
			Downloaded:  t.DownloadedEver,
			Uploaded:    t.UploadedEver,
			Ratio:       t.UploadRatio,
			UploadSpeed: t.RateUpload,
			DownSpeed:   t.RateDownload,
			ETA:         t.Eta,
			State:       state,
			Status:      status,
			Message:     t.ErrorString,
			Created:     t.AddedDate,
			Active:      t.Status == StatusDownload || t.Status == StatusSeed,
		}
	}

	return result, nil
}

// RemoveTorrent removes a torrent from Transmission
func (c *TransmissionClient) RemoveTorrent(hash string, removeData bool) error {
	// Create arguments
	args := TorrentRemoveArguments{
		IDs:             []string{hash},
		DeleteLocalData: removeData,
	}

	// Send request
	_, err := c.sendRequest(methodTorrentRemove, args)
	return err
}

// StartTorrent starts a stopped torrent
func (c *TransmissionClient) StartTorrent(hash string) error {
	// Create arguments
	args := struct {
		IDs []string `json:"ids"`
	}{
		IDs: []string{hash},
	}

	// Send request
	_, err := c.sendRequest(methodTorrentStart, args)
	return err
}

// StopTorrent stops a running torrent
func (c *TransmissionClient) StopTorrent(hash string) error {
	// Create arguments
	args := struct {
		IDs []string `json:"ids"`
	}{
		IDs: []string{hash},
	}

	// Send request
	_, err := c.sendRequest(methodTorrentStop, args)
	return err
}

// PauseTorrent pauses a running torrent (same as stop in Transmission)
func (c *TransmissionClient) PauseTorrent(hash string) error {
	return c.StopTorrent(hash)
}

// GetTorrentDetails gets detailed information about a specific torrent
func (c *TransmissionClient) GetTorrentDetails(hash string) (*TorrentInfo, error) {
	// Define fields to retrieve
	fields := []string{
		"id", "name", "hashString", "status", "error", "errorString",
		"percentDone", "rateDownload", "rateUpload", "sizeWhenDone",
		"totalSize", "uploadRatio", "eta", "downloadedEver", "uploadedEver",
		"addedDate", "isFinished", "isStalled", "leftUntilDone",
	}

	// Create arguments
	args := TorrentGetArguments{
		IDs:    []string{hash},
		Fields: fields,
	}

	// Send request
	resp, err := c.sendRequest(methodTorrentGet, args)
	if err != nil {
		return nil, err
	}

	// Parse response
	var getResp TorrentGetResponse
	if err := json.Unmarshal(resp.Arguments, &getResp); err != nil {
		return nil, fmt.Errorf("failed to parse torrent-get response: %w", err)
	}

	// Check if torrent was found
	if len(getResp.Torrents) == 0 {
		return nil, fmt.Errorf("torrent with hash %s not found", hash)
	}

	// Get the first (and only) torrent
	t := getResp.Torrents[0]

	// Convert status to a human-readable string
	status := c.getStatusString(t.Status)

	// Convert state
	state := c.getStateString(t.Status, t.Error, t.IsStalled)

	// Create TorrentInfo
	info := &TorrentInfo{
		Hash:        t.HashString,
		Name:        t.Name,
		Size:        t.TotalSize,
		Downloaded:  t.DownloadedEver,
		Uploaded:    t.UploadedEver,
		Ratio:       t.UploadRatio,
		UploadSpeed: t.RateUpload,
		DownSpeed:   t.RateDownload,
		ETA:         t.Eta,
		State:       state,
		Status:      status,
		Message:     t.ErrorString,
		Created:     t.AddedDate,
		Active:      t.Status == StatusDownload || t.Status == StatusSeed,
	}

	return info, nil
}

// getStatusString converts a Transmission status code to a human-readable string
func (c *TransmissionClient) getStatusString(status int) string {
	switch status {
	case StatusStopped:
		return "Stopped"
	case StatusCheckWait:
		return "Queued for verification"
	case StatusCheck:
		return "Verifying local data"
	case StatusDownloadWait:
		return "Queued for download"
	case StatusDownload:
		return "Downloading"
	case StatusSeedWait:
		return "Queued for seeding"
	case StatusSeed:
		return "Seeding"
	default:
		return "Unknown status: " + strconv.Itoa(status)
	}
}

// getStateString gets a state string that's compatible with other clients
func (c *TransmissionClient) getStateString(status int, error int, isStalled bool) string {
	if error != 0 {
		return "error"
	}

	switch status {
	case StatusStopped:
		return "stopped"
	case StatusCheckWait, StatusCheck:
		return "checking"
	case StatusDownloadWait:
		return "queued"
	case StatusDownload:
		if isStalled {
			return "idle"
		}
		return "downloading"
	case StatusSeedWait:
		return "queued_up"
	case StatusSeed:
		if isStalled {
			return "finished"
		}
		return "seeding"
	default:
		return "unknown"
	}
}

// CheckHealth verifies the connection to Transmission is working
func (c *TransmissionClient) CheckHealth() (bool, string) {
	// Try to get session ID to check connectivity
	if err := c.getSessionID(); err != nil {
		return false, fmt.Sprintf("Failed to connect: %s", err.Error())
	}

	return true, "Connected successfully"
}

// GetClientInfo returns information about the Transmission client
func (c *TransmissionClient) GetClientInfo() (*TorrentClientInfo, error) {
	// Get session info to retrieve version and other details
	resp, err := c.sendRequest(methodSessionGet, nil)
	if err != nil {
		return nil, fmt.Errorf("failed to get session info: %w", err)
	}

	// Parse session info
	var sessionInfo struct {
		Version         string `json:"version"`
		RpcVersion      int    `json:"rpc-version"`
		DownloadDirFree int64  `json:"download-dir-free-space"`
	}
	if err := json.Unmarshal(resp.Arguments, &sessionInfo); err != nil {
		return nil, fmt.Errorf("failed to parse session info: %w", err)
	}

	// Get session stats to retrieve global speeds
	statsResp, err := c.sendRequest(methodSessionStats, nil)
	if err != nil {
		return nil, fmt.Errorf("failed to get session stats: %w", err)
	}

	// Parse session stats
	var sessionStats struct {
		CurrentStats struct {
			DownloadedBytes int64 `json:"downloadedBytes"`
			UploadedBytes   int64 `json:"uploadedBytes"`
			FilesAdded      int   `json:"filesAdded"`
		} `json:"current-stats"`
		TorrentCount  int   `json:"torrentCount"`
		DownloadSpeed int64 `json:"downloadSpeed"`
		UploadSpeed   int64 `json:"uploadSpeed"`
	}
	if err := json.Unmarshal(statsResp.Arguments, &sessionStats); err != nil {
		return nil, fmt.Errorf("failed to parse session stats: %w", err)
	}

	// Create client info
	info := &TorrentClientInfo{
		ClientType:      "Transmission",
		Version:         sessionInfo.Version,
		ConnectedStatus: true,
		Message:         fmt.Sprintf("Connected to Transmission %s (RPC v%d)", sessionInfo.Version, sessionInfo.RpcVersion),
		DownloadSpeed:   sessionStats.DownloadSpeed,
		UploadSpeed:     sessionStats.UploadSpeed,
		TorrentsCount:   sessionStats.TorrentCount,
	}

	return info, nil
}

// GetTorrentFiles returns the list of files for a specific torrent
func (c *TransmissionClient) GetTorrentFiles(hash string) ([]string, error) {
	// Define fields to retrieve
	fields := []string{"files"}

	// Create arguments
	args := TorrentGetArguments{
		IDs:    []string{hash},
		Fields: fields,
	}

	// Send request
	resp, err := c.sendRequest(methodTorrentGet, args)
	if err != nil {
		return nil, err
	}

	// Parse response
	var getResp struct {
		Torrents []struct {
			Files []struct {
				Name   string `json:"name"`
				Length int64  `json:"length"`
			} `json:"files"`
		} `json:"torrents"`
	}
	if err := json.Unmarshal(resp.Arguments, &getResp); err != nil {
		return nil, fmt.Errorf("failed to parse torrent-get response: %w", err)
	}

	// Check if torrent was found
	if len(getResp.Torrents) == 0 {
		return nil, fmt.Errorf("torrent with hash %s not found", hash)
	}

	// Extract file paths
	var files []string
	for _, f := range getResp.Torrents[0].Files {
		files = append(files, f.Name)
	}

	return files, nil
}
