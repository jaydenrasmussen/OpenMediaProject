package clients

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"mime/multipart"
	"net/http"
	"net/url"
	"strings"
	"time"
)

// SABnzbdClient is a client for the SABnzbd API.
// SABnzbd is a Usenet/NZB downloader; we adapt its queue API to the TorrentClient interface.
type SABnzbdClient struct {
	baseURL    string
	apiKey     string
	httpClient *http.Client
}

// NewSABnzbdClient creates a new SABnzbdClient.
func NewSABnzbdClient(baseURL, apiKey string) *SABnzbdClient {
	return &SABnzbdClient{
		baseURL: strings.TrimRight(baseURL, "/"),
		apiKey:  apiKey,
		httpClient: &http.Client{
			Timeout: 30 * time.Second,
		},
	}
}

// apiURL builds a SABnzbd API URL with common parameters.
func (c *SABnzbdClient) apiURL(params url.Values) string {
	params.Set("apikey", c.apiKey)
	params.Set("output", "json")
	return c.baseURL + "/api?" + params.Encode()
}

// AddTorrentFile uploads an NZB file to SABnzbd.
func (c *SABnzbdClient) AddTorrentFile(nzbData []byte, filename string, startNow bool) error {
	var buf bytes.Buffer
	w := multipart.NewWriter(&buf)

	fw, err := w.CreateFormFile("nzbfile", filename)
	if err != nil {
		return err
	}
	if _, err := fw.Write(nzbData); err != nil {
		return err
	}
	if err := w.WriteField("apikey", c.apiKey); err != nil {
		return err
	}
	if err := w.WriteField("output", "json"); err != nil {
		return err
	}
	if !startNow {
		if err := w.WriteField("priority", "-2"); err != nil { // -2 = paused
			return err
		}
	}
	w.Close()

	req, err := http.NewRequest(http.MethodPost, c.baseURL+"/api?mode=addfile", &buf)
	if err != nil {
		return err
	}
	req.Header.Set("Content-Type", w.FormDataContentType())

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()

	var result struct {
		Status bool   `json:"status"`
		Error  string `json:"error"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return fmt.Errorf("sabnzbd addfile: failed to parse response: %w", err)
	}
	if !result.Status {
		return fmt.Errorf("sabnzbd addfile failed: %s", result.Error)
	}
	return nil
}

// AddTorrentURL adds an NZB from a URL to SABnzbd.
func (c *SABnzbdClient) AddTorrentURL(nzbURL string, startNow bool) error {
	params := url.Values{"mode": {"addurl"}, "name": {nzbURL}}
	if !startNow {
		params.Set("priority", "-2") // -2 = paused
	}
	resp, err := c.httpClient.Get(c.apiURL(params))
	if err != nil {
		return err
	}
	defer resp.Body.Close()

	var result struct {
		Status bool   `json:"status"`
		Error  string `json:"error"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return fmt.Errorf("sabnzbd addurl: failed to parse response: %w", err)
	}
	if !result.Status {
		return fmt.Errorf("sabnzbd addurl failed: %s", result.Error)
	}
	return nil
}

// GetTorrents returns the SABnzbd download queue as TorrentInfo entries.
func (c *SABnzbdClient) GetTorrents() (map[string]TorrentInfo, error) {
	params := url.Values{"mode": {"queue"}}
	resp, err := c.httpClient.Get(c.apiURL(params))
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	var result struct {
		Queue struct {
			Slots []struct {
				NZBID      string `json:"nzo_id"`
				Filename   string `json:"filename"`
				MB         string `json:"mb"`
				MBLeft     string `json:"mbleft"`
				Speed      string `json:"speed"`
				ETA        string `json:"eta"`
				Status     string `json:"status"`
				Percentage string `json:"percentage"`
			} `json:"slots"`
		} `json:"queue"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, err
	}

	out := make(map[string]TorrentInfo, len(result.Queue.Slots))
	for _, s := range result.Queue.Slots {
		state := sabStateToCommon(s.Status)
		out[s.NZBID] = TorrentInfo{
			Hash:   s.NZBID,
			Name:   s.Filename,
			State:  state,
			Status: s.Status,
			Active: state == "downloading",
		}
	}
	return out, nil
}

// sabStateToCommon maps SABnzbd job statuses to our common state strings.
func sabStateToCommon(status string) string {
	switch strings.ToLower(status) {
	case "downloading":
		return "downloading"
	case "paused":
		return "stopped"
	case "completed", "extracting", "moving":
		return "finished"
	case "failed":
		return "error"
	default:
		return strings.ToLower(status)
	}
}

// RemoveTorrent deletes a job from the SABnzbd queue.
func (c *SABnzbdClient) RemoveTorrent(id string, _ bool) error {
	params := url.Values{"mode": {"queue"}, "name": {"delete"}, "value": {id}}
	resp, err := c.httpClient.Get(c.apiURL(params))
	if err != nil {
		return err
	}
	resp.Body.Close()
	return nil
}

// StartTorrent resumes a paused SABnzbd job.
func (c *SABnzbdClient) StartTorrent(id string) error {
	params := url.Values{"mode": {"queue"}, "name": {"resume"}, "value": {id}}
	resp, err := c.httpClient.Get(c.apiURL(params))
	if err != nil {
		return err
	}
	resp.Body.Close()
	return nil
}

// StopTorrent pauses a SABnzbd job.
func (c *SABnzbdClient) StopTorrent(id string) error {
	params := url.Values{"mode": {"queue"}, "name": {"pause"}, "value": {id}}
	resp, err := c.httpClient.Get(c.apiURL(params))
	if err != nil {
		return err
	}
	resp.Body.Close()
	return nil
}

// PauseTorrent is an alias for StopTorrent.
func (c *SABnzbdClient) PauseTorrent(id string) error {
	return c.StopTorrent(id)
}

// GetTorrentDetails returns info for a single SABnzbd job.
func (c *SABnzbdClient) GetTorrentDetails(id string) (*TorrentInfo, error) {
	all, err := c.GetTorrents()
	if err != nil {
		return nil, err
	}
	info, ok := all[id]
	if !ok {
		return nil, fmt.Errorf("sabnzbd job %s not found", id)
	}
	return &info, nil
}

// CheckHealth verifies connectivity to SABnzbd.
func (c *SABnzbdClient) CheckHealth() (bool, string) {
	params := url.Values{"mode": {"version"}}
	resp, err := c.httpClient.Get(c.apiURL(params))
	if err != nil {
		return false, fmt.Sprintf("SABnzbd connection failed: %s", err.Error())
	}
	defer resp.Body.Close()
	return true, "Connected successfully"
}

// GetClientInfo returns version and queue stats from SABnzbd.
func (c *SABnzbdClient) GetClientInfo() (*TorrentClientInfo, error) {
	params := url.Values{"mode": {"version"}}
	resp, err := c.httpClient.Get(c.apiURL(params))
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	body, _ := io.ReadAll(resp.Body)
	var versionResp struct {
		Version string `json:"version"`
	}
	_ = json.Unmarshal(body, &versionResp)

	torrents, _ := c.GetTorrents()
	return &TorrentClientInfo{
		ClientType:      "SABnzbd",
		Version:         versionResp.Version,
		ConnectedStatus: true,
		TorrentsCount:   len(torrents),
	}, nil
}

// GetTorrentFiles returns the file list for a completed SABnzbd job (history).
func (c *SABnzbdClient) GetTorrentFiles(id string) ([]string, error) {
	params := url.Values{"mode": {"history"}, "nzo_ids": {id}}
	resp, err := c.httpClient.Get(c.apiURL(params))
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	var result struct {
		History struct {
			Slots []struct {
				Storage string `json:"storage"`
			} `json:"slots"`
		} `json:"history"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, err
	}
	var files []string
	for _, s := range result.History.Slots {
		if s.Storage != "" {
			files = append(files, s.Storage)
		}
	}
	return files, nil
}
