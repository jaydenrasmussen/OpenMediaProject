package clients

import (
	"bytes"
	"crypto/sha1"
	"encoding/base64"
	"encoding/hex"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"path/filepath"
	"regexp"
	"strings"
	"time"
)

// TorrentFile represents a torrent file with common metadata
type TorrentFile struct {
	Name         string   // Name of the torrent
	Hash         string   // Torrent info hash
	Size         int64    // Size in bytes
	Files        []string // List of files in the torrent
	Trackers     []string // List of trackers
	Date         int64    // Creation date (unix timestamp)
	DataBytes    []byte   // Raw torrent file data
	Base64Data   string   // Base64 encoded torrent data
	IsMagnet     bool     // Whether this is a magnet link
	MagnetURL    string   // Original magnet URL if applicable
	OriginalName string   // Original filename if available
}

var (
	// Magnet link regex to extract hash
	magnetHashRegex = regexp.MustCompile(`xt=urn:btih:([a-fA-F0-9]+)`)
	// Magnet link regex to extract name
	magnetNameRegex = regexp.MustCompile(`dn=([^&]+)`)
	// Magnet link regex to extract trackers
	magnetTrackerRegex = regexp.MustCompile(`tr=([^&]+)`)
)

// ProcessTorrentFile processes raw torrent file data and extracts metadata
func ProcessTorrentFile(data []byte, filename string) (*TorrentFile, error) {
	torrent := &TorrentFile{
		DataBytes:    data,
		Base64Data:   base64.StdEncoding.EncodeToString(data),
		OriginalName: filename,
		Date:         time.Now().Unix(),
	}

	// Calculate SHA1 hash for the info section
	// Note: This is a simplified approach; a full implementation would parse the bencoded data
	h := sha1.New()
	h.Write(data)
	torrent.Hash = hex.EncodeToString(h.Sum(nil))

	// Use the filename as the name if nothing better is available
	torrent.Name = filepath.Base(filename)

	// Set size
	torrent.Size = int64(len(data))

	return torrent, nil
}

// ProcessMagnetLink processes a magnet link and extracts metadata
func ProcessMagnetLink(magnetURL string) (*TorrentFile, error) {
	torrent := &TorrentFile{
		IsMagnet:  true,
		MagnetURL: magnetURL,
		Date:      time.Now().Unix(),
	}

	// Extract hash
	hashMatches := magnetHashRegex.FindStringSubmatch(magnetURL)
	if len(hashMatches) > 1 {
		torrent.Hash = strings.ToLower(hashMatches[1])
	} else {
		return nil, fmt.Errorf("invalid magnet link: missing hash")
	}

	// Extract name
	nameMatches := magnetNameRegex.FindStringSubmatch(magnetURL)
	if len(nameMatches) > 1 {
		decodedName, err := url.QueryUnescape(nameMatches[1])
		if err == nil {
			torrent.Name = decodedName
		} else {
			torrent.Name = nameMatches[1]
		}
	} else {
		torrent.Name = "Unnamed Torrent"
	}

	// Extract trackers
	trackerMatches := magnetTrackerRegex.FindAllStringSubmatch(magnetURL, -1)
	for _, match := range trackerMatches {
		if len(match) > 1 {
			decodedTracker, err := url.QueryUnescape(match[1])
			if err == nil {
				torrent.Trackers = append(torrent.Trackers, decodedTracker)
			} else {
				torrent.Trackers = append(torrent.Trackers, match[1])
			}
		}
	}

	return torrent, nil
}

// ProcessTorrentURL fetches a torrent file from a URL and processes it
func ProcessTorrentURL(torrentURL string) (*TorrentFile, error) {
	// Check if it's a magnet link
	if strings.HasPrefix(torrentURL, "magnet:") {
		return ProcessMagnetLink(torrentURL)
	}

	// Create HTTP client
	client := &http.Client{
		Timeout: 30 * time.Second,
	}

	// Create request
	req, err := http.NewRequest("GET", torrentURL, nil)
	if err != nil {
		return nil, fmt.Errorf("failed to create request: %w", err)
	}

	// Set user agent
	req.Header.Set("User-Agent", "OpenMediaProject Torrent Client")

	// Send request
	resp, err := client.Do(req)
	if err != nil {
		return nil, fmt.Errorf("failed to download torrent: %w", err)
	}
	defer resp.Body.Close()

	// Check response status
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("unexpected status code: %d", resp.StatusCode)
	}

	// Get suggested filename from content-disposition header
	filename := getFilenameFromHeader(resp.Header)
	if filename == "" {
		filename = filepath.Base(torrentURL)
		if filename == "" || filename == "." {
			filename = "downloaded.torrent"
		}
	}

	// Read body
	data, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("failed to read response body: %w", err)
	}

	// Process torrent file
	return ProcessTorrentFile(data, filename)
}

// getFilenameFromHeader extracts filename from Content-Disposition header
func getFilenameFromHeader(header http.Header) string {
	contentDisposition := header.Get("Content-Disposition")
	if contentDisposition == "" {
		return ""
	}

	// Extract filename
	filenameRegex := regexp.MustCompile(`filename=["']?([^"']+)["']?`)
	matches := filenameRegex.FindStringSubmatch(contentDisposition)
	if len(matches) > 1 {
		return matches[1]
	}

	return ""
}

// IsTorrentFile checks if data is likely a .torrent file
func IsTorrentFile(data []byte) bool {
	// A simple check - most torrent files start with "d8:announce"
	// This is not foolproof but a good quick check
	return bytes.HasPrefix(data, []byte("d8:announce")) ||
		bytes.HasPrefix(data, []byte("d7:comment")) ||
		bytes.HasPrefix(data, []byte("d10:created by"))
}

// IsMagnetLink checks if a string is a magnet link
func IsMagnetLink(url string) bool {
	return strings.HasPrefix(url, "magnet:")
}
