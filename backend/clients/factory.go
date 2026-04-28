package clients

import (
	"fmt"
)

// TorrentClientType represents the type of torrent client
type TorrentClientType string

// Available torrent client types
const (
	ClientTypeRuTorrent    TorrentClientType = "rutorrent"
	ClientTypeTransmission TorrentClientType = "transmission"
	ClientTypeDeluge       TorrentClientType = "deluge"
	ClientTypeQBittorrent  TorrentClientType = "qbittorrent"
	ClientTypeSABnzbd      TorrentClientType = "sabnzbd"
)

// TorrentClientConfig holds the common configuration for all torrent clients
type TorrentClientConfig struct {
	ClientType TorrentClientType
	BaseURL    string
	Username   string
	Password   string
}

// NewTorrentClient creates a new torrent client based on configuration
func NewTorrentClient(config TorrentClientConfig) (TorrentClient, error) {
	switch config.ClientType {
	case ClientTypeRuTorrent:
		return NewRuTorrentClient(config.BaseURL, config.Username, config.Password), nil
	case ClientTypeTransmission:
		return NewTransmissionClient(config.BaseURL, config.Username, config.Password), nil
	case ClientTypeDeluge:
		return NewDelugeClient(config.BaseURL, config.Password), nil
	case ClientTypeQBittorrent:
		return NewQBittorrentClient(config.BaseURL, config.Username, config.Password), nil
	case ClientTypeSABnzbd:
		return NewSABnzbdClient(config.BaseURL, config.Password), nil
	default:
		return nil, fmt.Errorf("unsupported torrent client type: %s", config.ClientType)
	}
}
