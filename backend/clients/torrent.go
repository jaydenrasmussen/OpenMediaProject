package clients

import (
	"net/http"
	"time"
)

// TorrentInfo represents generic information about a torrent across different clients
type TorrentInfo struct {
	Hash        string  `json:"hash"`
	Name        string  `json:"name"`
	Size        int64   `json:"size"`
	Downloaded  int64   `json:"downloaded"`
	Uploaded    int64   `json:"uploaded"`
	Ratio       float64 `json:"ratio"`
	UploadSpeed int64   `json:"upspeed"`
	DownSpeed   int64   `json:"downspeed"`
	ETA         int     `json:"eta"`
	State       string  `json:"state"`
	Status      string  `json:"status"`
	Message     string  `json:"message"`
	Created     int64   `json:"created"`
	Active      bool    `json:"active"`
}

// TorrentClientInfo represents information about a torrent client
type TorrentClientInfo struct {
	ClientType      string `json:"client_type"`
	Version         string `json:"version"`
	ConnectedStatus bool   `json:"connected"`
	Message         string `json:"message,omitempty"`
	DownloadSpeed   int64  `json:"download_speed,omitempty"` // Global download speed in bytes/s
	UploadSpeed     int64  `json:"upload_speed,omitempty"`   // Global upload speed in bytes/s
	TorrentsCount   int    `json:"torrents_count,omitempty"` // Number of torrents in the client
}

// TorrentClient defines a common interface for all torrent client implementations
type TorrentClient interface {
	// AddTorrentFile uploads a torrent file to the client
	AddTorrentFile(torrentData []byte, filename string, startNow bool) error

	// AddTorrentURL adds a torrent from a URL to the client
	AddTorrentURL(torrentURL string, startNow bool) error

	// GetTorrents retrieves the list of torrents from the client
	GetTorrents() (map[string]TorrentInfo, error)

	// RemoveTorrent removes a torrent from the client
	RemoveTorrent(hash string, removeData bool) error

	// StartTorrent starts a stopped torrent
	StartTorrent(hash string) error

	// StopTorrent stops a running torrent
	StopTorrent(hash string) error

	// PauseTorrent pauses a running torrent
	PauseTorrent(hash string) error

	// GetTorrentDetails gets detailed information about a specific torrent
	GetTorrentDetails(hash string) (*TorrentInfo, error)

	// CheckHealth verifies the connection to the client is working
	CheckHealth() (bool, string)

	// GetClientInfo returns information about the client
	GetClientInfo() (*TorrentClientInfo, error)

	// GetTorrentFiles returns the list of files for a specific torrent
	GetTorrentFiles(hash string) ([]string, error)
}

// HTTPClientProvider provides a consistent way to create HTTP clients
func NewHTTPClient() *http.Client {
	return &http.Client{
		Timeout: 30 * time.Second,
	}
}
