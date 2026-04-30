package clients

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"mime/multipart"
	"net/http"
	"net/http/cookiejar"
	"net/url"
	"strings"
	"time"
)

// QBittorrentClient is a client for the qBittorrent Web API v2.
type QBittorrentClient struct {
	baseURL    string
	username   string
	password   string
	httpClient *http.Client
}

// NewQBittorrentClient creates a new QBittorrentClient.
func NewQBittorrentClient(baseURL, username, password string) *QBittorrentClient {
	jar, _ := cookiejar.New(nil)
	return &QBittorrentClient{
		baseURL:  strings.TrimRight(baseURL, "/"),
		username: username,
		password: password,
		httpClient: &http.Client{
			Timeout: 30 * time.Second,
			Jar:     jar,
		},
	}
}

// login authenticates with the qBittorrent Web UI.
func (c *QBittorrentClient) login() error {
	form := url.Values{"username": {c.username}, "password": {c.password}}
	resp, err := c.httpClient.PostForm(c.baseURL+"/api/v2/auth/login", form)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	body, _ := io.ReadAll(resp.Body)
	if strings.TrimSpace(string(body)) != "Ok." {
		return fmt.Errorf("qBittorrent login failed: %s", string(body))
	}
	return nil
}

// doJSON is a helper that performs a GET request and decodes JSON.
func (c *QBittorrentClient) doJSON(urlPath string, out interface{}) error {
	resp, err := c.httpClient.Get(c.baseURL + urlPath)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	return json.NewDecoder(resp.Body).Decode(out)
}

// doForm posts form values to the API and returns an error on non-2xx status.
func (c *QBittorrentClient) doForm(urlPath string, values url.Values) error {
	resp, err := c.httpClient.PostForm(c.baseURL+urlPath, values)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		body, _ := io.ReadAll(resp.Body)
		return fmt.Errorf("qBittorrent %s: unexpected status %d: %s",
			urlPath, resp.StatusCode, strings.TrimSpace(string(body)))
	}
	return nil
}

// AddTorrentFile uploads a torrent file to qBittorrent.
func (c *QBittorrentClient) AddTorrentFile(torrentData []byte, filename string, startNow bool) error {
	if err := c.login(); err != nil {
		return err
	}

	var buf bytes.Buffer
	w := multipart.NewWriter(&buf)
	fw, err := w.CreateFormFile("torrents", filename)
	if err != nil {
		return err
	}
	if _, err := fw.Write(torrentData); err != nil {
		return err
	}
	if !startNow {
		_ = w.WriteField("paused", "true")
	}
	w.Close()

	req, err := http.NewRequest(http.MethodPost, c.baseURL+"/api/v2/torrents/add", &buf)
	if err != nil {
		return err
	}
	req.Header.Set("Content-Type", w.FormDataContentType())

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	body, _ := io.ReadAll(resp.Body)
	if strings.TrimSpace(string(body)) != "Ok." {
		return fmt.Errorf("qBittorrent addTorrentFile failed: %s", string(body))
	}
	return nil
}

// AddTorrentURL adds a torrent from a URL or magnet link.
func (c *QBittorrentClient) AddTorrentURL(torrentURL string, startNow bool) error {
	if err := c.login(); err != nil {
		return err
	}
	form := url.Values{"urls": {torrentURL}}
	if !startNow {
		form.Set("paused", "true")
	}
	resp, err := c.httpClient.PostForm(c.baseURL+"/api/v2/torrents/add", form)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	body, _ := io.ReadAll(resp.Body)
	if strings.TrimSpace(string(body)) != "Ok." {
		return fmt.Errorf("qBittorrent addTorrentURL failed: %s", string(body))
	}
	return nil
}

// GetTorrents returns all torrents from qBittorrent.
func (c *QBittorrentClient) GetTorrents() (map[string]TorrentInfo, error) {
	if err := c.login(); err != nil {
		return nil, err
	}

	var raw []struct {
		Hash         string  `json:"hash"`
		Name         string  `json:"name"`
		Size         int64   `json:"size"`
		Downloaded   int64   `json:"downloaded"`
		Uploaded     int64   `json:"uploaded"`
		Ratio        float64 `json:"ratio"`
		UploadSpeed  int64   `json:"upspeed"`
		DownloadSpeed int64  `json:"dlspeed"`
		ETA          int     `json:"eta"`
		State        string  `json:"state"`
		AddedOn      int64   `json:"added_on"`
	}
	if err := c.doJSON("/api/v2/torrents/info", &raw); err != nil {
		return nil, err
	}

	result := make(map[string]TorrentInfo, len(raw))
	for _, t := range raw {
		state := qbStateToCommon(t.State)
		result[t.Hash] = TorrentInfo{
			Hash:        t.Hash,
			Name:        t.Name,
			Size:        t.Size,
			Downloaded:  t.Downloaded,
			Uploaded:    t.Uploaded,
			Ratio:       t.Ratio,
			UploadSpeed: t.UploadSpeed,
			DownSpeed:   t.DownloadSpeed,
			ETA:         t.ETA,
			State:       state,
			Status:      t.State,
			Created:     t.AddedOn,
			Active:      state == "downloading" || state == "seeding",
		}
	}
	return result, nil
}

// qbStateToCommon maps qBittorrent states to our common state strings.
func qbStateToCommon(state string) string {
	switch state {
	case "uploading", "stalledUP", "forcedUP", "queuedUP":
		return "seeding"
	case "downloading", "stalledDL", "forcedDL", "queuedDL":
		return "downloading"
	case "pausedDL":
		return "stopped"
	case "pausedUP":
		return "finished"
	case "checkingDL", "checkingUP", "checkingResumeData":
		return "checking"
	case "error", "missingFiles":
		return "error"
	default:
		return state
	}
}

// RemoveTorrent removes a torrent from qBittorrent.
func (c *QBittorrentClient) RemoveTorrent(hash string, removeData bool) error {
	if err := c.login(); err != nil {
		return err
	}
	deleteFiles := "false"
	if removeData {
		deleteFiles = "true"
	}
	return c.doForm("/api/v2/torrents/delete", url.Values{
		"hashes":      {hash},
		"deleteFiles": {deleteFiles},
	})
}

// StartTorrent resumes a paused torrent.
func (c *QBittorrentClient) StartTorrent(hash string) error {
	if err := c.login(); err != nil {
		return err
	}
	return c.doForm("/api/v2/torrents/resume", url.Values{"hashes": {hash}})
}

// StopTorrent pauses a torrent.
func (c *QBittorrentClient) StopTorrent(hash string) error {
	if err := c.login(); err != nil {
		return err
	}
	return c.doForm("/api/v2/torrents/pause", url.Values{"hashes": {hash}})
}

// PauseTorrent is an alias for StopTorrent.
func (c *QBittorrentClient) PauseTorrent(hash string) error {
	return c.StopTorrent(hash)
}

// GetTorrentDetails returns info for a single torrent.
func (c *QBittorrentClient) GetTorrentDetails(hash string) (*TorrentInfo, error) {
	all, err := c.GetTorrents()
	if err != nil {
		return nil, err
	}
	info, ok := all[hash]
	if !ok {
		return nil, fmt.Errorf("torrent %s not found", hash)
	}
	return &info, nil
}

// CheckHealth verifies connectivity to qBittorrent.
func (c *QBittorrentClient) CheckHealth() (bool, string) {
	if err := c.login(); err != nil {
		return false, fmt.Sprintf("qBittorrent connection failed: %s", err.Error())
	}
	return true, "Connected successfully"
}

// GetClientInfo returns version and stats from qBittorrent.
func (c *QBittorrentClient) GetClientInfo() (*TorrentClientInfo, error) {
	if err := c.login(); err != nil {
		return nil, err
	}

	resp, err := c.httpClient.Get(c.baseURL + "/api/v2/app/version")
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	versionBytes, _ := io.ReadAll(resp.Body)
	version := strings.TrimSpace(string(versionBytes))

	var stats struct {
		DlInfoSpeed int64 `json:"dl_info_speed"`
		UpInfoSpeed int64 `json:"up_info_speed"`
	}
	if err := c.doJSON("/api/v2/transfer/info", &stats); err != nil {
		stats.DlInfoSpeed = 0
		stats.UpInfoSpeed = 0
	}

	torrents, _ := c.GetTorrents()
	return &TorrentClientInfo{
		ClientType:    "qBittorrent",
		Version:       version,
		ConnectedStatus: true,
		DownloadSpeed: stats.DlInfoSpeed,
		UploadSpeed:   stats.UpInfoSpeed,
		TorrentsCount: len(torrents),
	}, nil
}

// GetTorrentFiles returns the file paths inside a torrent.
func (c *QBittorrentClient) GetTorrentFiles(hash string) ([]string, error) {
	if err := c.login(); err != nil {
		return nil, err
	}

	var raw []struct {
		Name string `json:"name"`
	}
	if err := c.doJSON("/api/v2/torrents/files?hash="+hash, &raw); err != nil {
		return nil, err
	}

	files := make([]string, 0, len(raw))
	for _, f := range raw {
		files = append(files, f.Name)
	}
	return files, nil
}
