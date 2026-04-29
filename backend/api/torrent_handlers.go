package api

import (
	"backend/clients/notifications"
	"bytes"
	"fmt"
	"io"
	"net/url"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"sync"
	"time"

	"backend/clients"
	"backend/config"
	"backend/db"
	"backend/models"

	"github.com/gofiber/fiber/v2"
	"github.com/rs/zerolog/log"
)

// TorrentClientHealthResponse represents the response for the torrent client health check
type TorrentClientHealthResponse struct {
	ClientType    string `json:"client_type"`
	Version       string `json:"version"`
	Connected     bool   `json:"connected"`
	Message       string `json:"message,omitempty"`
	DownloadSpeed int64  `json:"download_speed,omitempty"`
	UploadSpeed   int64  `json:"upload_speed,omitempty"`
	TorrentsCount int    `json:"torrents_count,omitempty"`
	ResponseTime  int64  `json:"response_time_ms,omitempty"`
	LastChecked   string `json:"last_checked,omitempty"`
}

// TorrentUploadResponse represents the response after uploading a torrent
type TorrentUploadResponse struct {
	Success bool   `json:"success"`
	Message string `json:"message"`
}

// TorrentListResponse represents the response for listing torrents
type TorrentListResponse struct {
	Torrents []TorrentInfo `json:"torrents"`
	Count    int           `json:"count"`
}

// TorrentInfo represents torrent information for API responses
type TorrentInfo struct {
	Hash        string  `json:"hash"`
	Name        string  `json:"name"`
	Size        int64   `json:"size"`
	Downloaded  int64   `json:"downloaded"`
	Uploaded    int64   `json:"uploaded"`
	Ratio       float64 `json:"ratio"`
	UploadSpeed int64   `json:"upload_speed"`
	DownSpeed   int64   `json:"download_speed"`
	ETA         int     `json:"eta"`
	State       string  `json:"state"`
	Status      string  `json:"status"`
	Progress    float64 `json:"progress"`
	Created     int64   `json:"created"`
	Active      bool    `json:"active"`
	Message     string  `json:"message,omitempty"`
}

// TorrentHandler holds the dependencies for the torrent handlers.
type TorrentHandler struct {
	mediaServers  *clients.MediaServerRegistry
	fileMover     *clients.FileMover
	config        *config.Config
	notifRegistry *notifications.Registry
}

// NewTorrentHandler creates a new TorrentHandler.
func NewTorrentHandler(mediaServers *clients.MediaServerRegistry, fileMover *clients.FileMover, config *config.Config, notifRegistry *notifications.Registry) *TorrentHandler {
	return &TorrentHandler{
		mediaServers:  mediaServers,
		fileMover:     fileMover,
		config:        config,
		notifRegistry: notifRegistry,
	}
}

// UploadTorrent handles uploading a torrent file to ruTorrent
func (h *TorrentHandler) UploadTorrent(c *fiber.Ctx) error {
	// Get configuration
	cfg := h.config

	// Check if a torrent client is configured
	if cfg.TorrentClient.BaseURL == "" {
		// Fallback to legacy ruTorrent config
		if cfg.RuTorrent.BaseURL == "" {
			return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
				"error":   true,
				"message": "No torrent client URL is configured",
			})
		}

		// Use legacy ruTorrent config
		cfg.TorrentClient.ClientType = config.ClientTypeRuTorrent
		cfg.TorrentClient.BaseURL = cfg.RuTorrent.BaseURL
		cfg.TorrentClient.Username = cfg.RuTorrent.Username
		cfg.TorrentClient.Password = cfg.RuTorrent.Password
	}

	// Get start parameter (default is true - start torrent after adding)
	startParam := c.Query("start", "true")
	startTorrent := true
	if strings.ToLower(startParam) == "false" {
		startTorrent = false
	}

	// Check if there's a URL or magnet parameter
	torrentURL := c.FormValue("torrent_url")
	if torrentURL != "" {
		return h.addTorrentFromURL(c, torrentURL, startTorrent)
	}

	// Get the multipart form
	file, err := c.FormFile("torrent_file")
	if err != nil {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "No torrent file or URL provided: " + err.Error(),
		})
	}

	// Open the file
	openedFile, err := file.Open()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to open uploaded file: " + err.Error(),
		})
	}
	defer openedFile.Close()

	// Read the file content
	buf := bytes.NewBuffer(nil)
	if _, err := io.Copy(buf, openedFile); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to read uploaded file: " + err.Error(),
		})
	}

	// Verify it's a torrent file
	if !clients.IsTorrentFile(buf.Bytes()) {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "The uploaded file is not a valid torrent file",
		})
	}

	// Create torrent client
	clientConfig := clients.TorrentClientConfig{
		ClientType: clients.TorrentClientType(cfg.TorrentClient.ClientType),
		BaseURL:    cfg.TorrentClient.BaseURL,
		Username:   cfg.TorrentClient.Username,
		Password:   cfg.TorrentClient.Password,
	}

	client, err := clients.NewTorrentClient(clientConfig)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to create torrent client: " + err.Error(),
		})
	}

	// Add the torrent file
	err = client.AddTorrentFile(buf.Bytes(), file.Filename, startTorrent)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to add torrent to client: " + err.Error(),
		})
	}

	if h.mediaServers != nil {
		h.mediaServers.Scan()
	}

	_ = db.NewHistoryRepository().Add(&models.HistoryEntry{
		EventType: models.HistoryEventTorrentAdded,
		Title:     file.Filename,
		Message:   fmt.Sprintf("Torrent manually added: %s", file.Filename),
	})

	return c.JSON(TorrentUploadResponse{
		Success: true,
		Message: fmt.Sprintf("Torrent '%s' successfully added", file.Filename),
	})
}

// addTorrentFromURL handles adding a torrent from a URL or magnet link
// addTorrentFromURL handles adding a torrent from a URL or magnet link
func (h *TorrentHandler) addTorrentFromURL(c *fiber.Ctx, torrentURL string, startTorrent bool) error {
	// Create torrent client
	clientConfig := clients.TorrentClientConfig{
		ClientType: clients.TorrentClientType(h.config.TorrentClient.ClientType),
		BaseURL:    h.config.TorrentClient.BaseURL,
		Username:   h.config.TorrentClient.Username,
		Password:   h.config.TorrentClient.Password,
	}

	client, err := clients.NewTorrentClient(clientConfig)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to create torrent client: " + err.Error(),
		})
	}

	// Check if it's a magnet link
	if clients.IsMagnetLink(torrentURL) {
		// Process magnet link
		torrentInfo, err := clients.ProcessMagnetLink(torrentURL)
		if err != nil {
			return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
				"error":   true,
				"message": "Invalid magnet link: " + err.Error(),
			})
		}

		// Add the magnet link
		err = client.AddTorrentURL(torrentURL, startTorrent)
		if err != nil {
			return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
				"error":   true,
				"message": "Failed to add magnet link: " + err.Error(),
			})
		}

		if h.mediaServers != nil {
			h.mediaServers.Scan()
		}

		return c.JSON(TorrentUploadResponse{
			Success: true,
			Message: fmt.Sprintf("Magnet link for '%s' successfully added", torrentInfo.Name),
		})
	}

	// Validate URL
	_, err = url.ParseRequestURI(torrentURL)
	if err != nil {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Invalid URL: " + err.Error(),
		})
	}

	// Process torrent URL
	torrentInfo, err := clients.ProcessTorrentURL(torrentURL)
	if err != nil {
		// If we can't process it ourselves, let the client try directly
		err = client.AddTorrentURL(torrentURL, startTorrent)
		if err != nil {
			return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
				"error":   true,
				"message": "Failed to add torrent URL: " + err.Error(),
			})
		}

		if h.mediaServers != nil {
			h.mediaServers.Scan()
		}

		return c.JSON(TorrentUploadResponse{
			Success: true,
			Message: fmt.Sprintf("Torrent from URL '%s' successfully added", torrentURL),
		})
	}

	// If we got the torrent file successfully, add it as a file
	err = client.AddTorrentFile(torrentInfo.DataBytes, torrentInfo.Name, startTorrent)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to add torrent file from URL: " + err.Error(),
		})
	}

	if h.mediaServers != nil {
		h.mediaServers.Scan()
	}

	return c.JSON(TorrentUploadResponse{
		Success: true,
		Message: fmt.Sprintf("Torrent '%s' from URL successfully added", torrentInfo.Name),
	})
}

// GetTorrents returns the list of torrents from ruTorrent
func (h *TorrentHandler) GetTorrents(c *fiber.Ctx) error {
	// Get configuration
	cfg := h.config

	// Check if a torrent client is configured
	if cfg.TorrentClient.BaseURL == "" {
		// Fallback to legacy ruTorrent config
		if cfg.RuTorrent.BaseURL == "" {
			return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
				"error":   true,
				"message": "No torrent client URL is configured",
			})
		}

		// Use legacy ruTorrent config
		cfg.TorrentClient.ClientType = config.ClientTypeRuTorrent
		cfg.TorrentClient.BaseURL = cfg.RuTorrent.BaseURL
		cfg.TorrentClient.Username = cfg.RuTorrent.Username
		cfg.TorrentClient.Password = cfg.RuTorrent.Password
	}

	// Create torrent client
	clientConfig := clients.TorrentClientConfig{
		ClientType: clients.TorrentClientType(cfg.TorrentClient.ClientType),
		BaseURL:    cfg.TorrentClient.BaseURL,
		Username:   cfg.TorrentClient.Username,
		Password:   cfg.TorrentClient.Password,
	}

	client, err := clients.NewTorrentClient(clientConfig)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to create torrent client: " + err.Error(),
		})
	}

	// Get the torrents
	torrents, err := client.GetTorrents()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to get torrents: " + err.Error(),
		})
	}

	// Convert to our API response format
	torrentList := make([]TorrentInfo, 0, len(torrents))
	for hash, torrent := range torrents {
		progress := 0.0
		if torrent.Size > 0 {
			progress = float64(torrent.Downloaded) / float64(torrent.Size) * 100
		}

		torrentList = append(torrentList, TorrentInfo{
			Hash:        hash,
			Name:        torrent.Name,
			Size:        torrent.Size,
			Downloaded:  torrent.Downloaded,
			Uploaded:    torrent.Uploaded,
			Ratio:       torrent.Ratio,
			UploadSpeed: torrent.UploadSpeed,
			DownSpeed:   torrent.DownSpeed,
			ETA:         torrent.ETA,
			State:       torrent.State,
			Status:      torrent.Status,
			Progress:    progress,
			Created:     torrent.Created,
			Active:      torrent.Active,
		})
	}

	return c.JSON(TorrentListResponse{
		Torrents: torrentList,
		Count:    len(torrentList),
	})
}

// DeleteTorrent removes a torrent from ruTorrent
func (h *TorrentHandler) DeleteTorrent(c *fiber.Ctx) error {
	hash := c.Params("hash")
	if hash == "" {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Torrent hash is required",
		})
	}

	// Get "with_data" parameter (default is false)
	withDataParam := c.Query("with_data", "false")
	withData := false
	if strings.ToLower(withDataParam) == "true" {
		withData = true
	}

	// Get configuration
	cfg := h.config

	// Check if a torrent client is configured
	if cfg.TorrentClient.BaseURL == "" {
		// Fallback to legacy ruTorrent config
		if cfg.RuTorrent.BaseURL == "" {
			return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
				"error":   true,
				"message": "No torrent client URL is configured",
			})
		}

		// Use legacy ruTorrent config
		cfg.TorrentClient.ClientType = config.ClientTypeRuTorrent
		cfg.TorrentClient.BaseURL = cfg.RuTorrent.BaseURL
		cfg.TorrentClient.Username = cfg.RuTorrent.Username
		cfg.TorrentClient.Password = cfg.RuTorrent.Password
	}

	// Create torrent client
	clientConfig := clients.TorrentClientConfig{
		ClientType: clients.TorrentClientType(cfg.TorrentClient.ClientType),
		BaseURL:    cfg.TorrentClient.BaseURL,
		Username:   cfg.TorrentClient.Username,
		Password:   cfg.TorrentClient.Password,
	}

	client, err := clients.NewTorrentClient(clientConfig)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to create torrent client: " + err.Error(),
		})
	}

	// Remove the torrent
	err = client.RemoveTorrent(hash, withData)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to remove torrent: " + err.Error(),
		})
	}

	return c.JSON(fiber.Map{
		"success": true,
		"message": fmt.Sprintf("Torrent with hash '%s' successfully removed", hash),
	})
}

// ControlTorrent handles start/stop/pause operations on a torrent
func (h *TorrentHandler) ControlTorrent(c *fiber.Ctx) error {
	hash := c.Params("hash")
	if hash == "" {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Torrent hash is required",
		})
	}

	// Get action parameter
	action := c.Params("action")
	if action == "" {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Action is required (start, stop, or pause)",
		})
	}

	// Validate action
	if action != "start" && action != "stop" && action != "pause" {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Invalid action. Must be start, stop, or pause",
		})
	}

	// Get configuration
	cfg := h.config

	// Check if a torrent client is configured
	if cfg.TorrentClient.BaseURL == "" {
		// Fallback to legacy ruTorrent config
		if cfg.RuTorrent.BaseURL == "" {
			return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
				"error":   true,
				"message": "No torrent client URL is configured",
			})
		}

		// Use legacy ruTorrent config
		cfg.TorrentClient.ClientType = config.ClientTypeRuTorrent
		cfg.TorrentClient.BaseURL = cfg.RuTorrent.BaseURL
		cfg.TorrentClient.Username = cfg.RuTorrent.Username
		cfg.TorrentClient.Password = cfg.RuTorrent.Password
	}

	// Create torrent client
	clientConfig := clients.TorrentClientConfig{
		ClientType: clients.TorrentClientType(cfg.TorrentClient.ClientType),
		BaseURL:    cfg.TorrentClient.BaseURL,
		Username:   cfg.TorrentClient.Username,
		Password:   cfg.TorrentClient.Password,
	}

	client, err := clients.NewTorrentClient(clientConfig)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to create torrent client: " + err.Error(),
		})
	}

	// Perform the action
	var actionErr error
	switch action {
	case "start":
		actionErr = client.StartTorrent(hash)
	case "stop":
		actionErr = client.StopTorrent(hash)
	case "pause":
		actionErr = client.PauseTorrent(hash)
	}

	if actionErr != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": fmt.Sprintf("Failed to %s torrent: %s", action, actionErr.Error()),
		})
	}

	// For consistency in response messages
	actionPast := action
	if action == "pause" {
		actionPast = "paused"
	} else {
		actionPast = action + "ed"
	}

	return c.JSON(fiber.Map{
		"success": true,
		"message": fmt.Sprintf("Torrent with hash '%s' successfully %s", hash, actionPast),
	})
}

// GetTorrentDetails returns detailed information about a specific torrent
func (h *TorrentHandler) GetTorrentDetails(c *fiber.Ctx) error {
	hash := c.Params("hash")
	if hash == "" {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Torrent hash is required",
		})
	}

	// Get configuration
	cfg := h.config

	// Check if a torrent client is configured
	if cfg.TorrentClient.BaseURL == "" {
		// Fallback to legacy ruTorrent config
		if cfg.RuTorrent.BaseURL == "" {
			return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
				"error":   true,
				"message": "No torrent client URL is configured",
			})
		}

		// Use legacy ruTorrent config
		cfg.TorrentClient.ClientType = config.ClientTypeRuTorrent
		cfg.TorrentClient.BaseURL = cfg.RuTorrent.BaseURL
		cfg.TorrentClient.Username = cfg.RuTorrent.Username
		cfg.TorrentClient.Password = cfg.RuTorrent.Password
	}

	// Create torrent client
	clientConfig := clients.TorrentClientConfig{
		ClientType: clients.TorrentClientType(cfg.TorrentClient.ClientType),
		BaseURL:    cfg.TorrentClient.BaseURL,
		Username:   cfg.TorrentClient.Username,
		Password:   cfg.TorrentClient.Password,
	}

	client, err := clients.NewTorrentClient(clientConfig)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to create torrent client: " + err.Error(),
		})
	}

	// Get the torrent details
	torrent, err := client.GetTorrentDetails(hash)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to get torrent details: " + err.Error(),
		})
	}

	// Calculate progress
	progress := 0.0
	if torrent.Size > 0 {
		progress = float64(torrent.Downloaded) / float64(torrent.Size) * 100
	}

	// Convert to our API response format
	torrentInfo := TorrentInfo{
		Hash:        hash,
		Name:        torrent.Name,
		Size:        torrent.Size,
		Downloaded:  torrent.Downloaded,
		Uploaded:    torrent.Uploaded,
		Ratio:       torrent.Ratio,
		UploadSpeed: torrent.UploadSpeed,
		DownSpeed:   torrent.DownSpeed,
		ETA:         torrent.ETA,
		State:       torrent.State,
		Status:      torrent.Status,
		Progress:    progress,
		Created:     torrent.Created,
		Active:      torrent.Active,
		Message:     torrent.Message,
	}

	return c.JSON(torrentInfo)
}

// GetClientHealth returns health information about the configured torrent client
func (h *TorrentHandler) GetClientHealth(c *fiber.Ctx) error {
	// Get configuration
	cfg := h.config

	// Check if a torrent client is configured
	if cfg.TorrentClient.BaseURL == "" {
		// Fallback to legacy ruTorrent config
		if cfg.RuTorrent.BaseURL == "" {
			return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
				"error":   true,
				"message": "No torrent client URL is configured",
			})
		}

		// Use legacy ruTorrent config
		cfg.TorrentClient.ClientType = config.ClientTypeRuTorrent
		cfg.TorrentClient.BaseURL = cfg.RuTorrent.BaseURL
		cfg.TorrentClient.Username = cfg.RuTorrent.Username
		cfg.TorrentClient.Password = cfg.RuTorrent.Password
	}

	// Create torrent client
	clientConfig := clients.TorrentClientConfig{
		ClientType: clients.TorrentClientType(cfg.TorrentClient.ClientType),
		BaseURL:    cfg.TorrentClient.BaseURL,
		Username:   cfg.TorrentClient.Username,
		Password:   cfg.TorrentClient.Password,
	}

	client, err := clients.NewTorrentClient(clientConfig)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to create torrent client: " + err.Error(),
		})
	}

	// Measure response time
	startTime := time.Now()

	// Check health in one goroutine
	var connected bool
	var message string
	var healthErr error
	var wg sync.WaitGroup
	wg.Add(1)
	go func() {
		defer wg.Done()
		connected, message = client.CheckHealth()
	}()

	// Get client info in another goroutine if health check passes
	var clientInfo *clients.TorrentClientInfo
	wg.Add(1)
	go func() {
		defer wg.Done()
		if clientInfo, healthErr = client.GetClientInfo(); healthErr != nil {
			message = fmt.Sprintf("%s; Failed to get client info: %s", message, healthErr.Error())
		}
	}()

	// Wait for both goroutines to finish
	wg.Wait()

	// Calculate response time
	responseTime := time.Since(startTime).Milliseconds()

	// Create response
	response := TorrentClientHealthResponse{
		Connected:    connected,
		Message:      message,
		ResponseTime: responseTime,
		LastChecked:  time.Now().Format(time.RFC3339),
	}

	// Add client info if available
	if clientInfo != nil {
		response.ClientType = clientInfo.ClientType
		response.Version = clientInfo.Version
		response.DownloadSpeed = clientInfo.DownloadSpeed
		response.UploadSpeed = clientInfo.UploadSpeed
		response.TorrentsCount = clientInfo.TorrentsCount
	} else {
		response.ClientType = string(cfg.TorrentClient.ClientType)
	}

	return c.JSON(response)
}

// CheckForCompletedTorrents checks for completed torrents and moves them to the appropriate directory.
func (h *TorrentHandler) CheckForCompletedTorrents() {
	// Create torrent client
	clientConfig := clients.TorrentClientConfig{
		ClientType: clients.TorrentClientType(h.config.TorrentClient.ClientType),
		BaseURL:    h.config.TorrentClient.BaseURL,
		Username:   h.config.TorrentClient.Username,
		Password:   h.config.TorrentClient.Password,
	}

	client, err := clients.NewTorrentClient(clientConfig)
	if err != nil {
		log.Warn().Err(err).Msg("CheckForCompletedTorrents: failed to create torrent client")
		return
	}

	// Get the torrents
	torrents, err := client.GetTorrents()
	if err != nil {
		log.Warn().Err(err).Msg("CheckForCompletedTorrents: failed to get torrents")
		return
	}

	downloadRepo := db.NewDownloadRepository()

	plexScanNeeded := false

	for hash, torrent := range torrents {
		if torrent.State != "seeding" && torrent.State != "finished" {
			continue
		}

		files, err := client.GetTorrentFiles(hash)
		if err != nil {
			log.Warn().Err(err).Str("hash", hash).Msg("CheckForCompletedTorrents: failed to get torrent files")
			continue
		}

		// Look up this torrent in the download tracking DB.
		download, dbErr := downloadRepo.GetByHash(hash)
		if dbErr != nil && dbErr != db.ErrDownloadNotFound {
			log.Warn().Err(dbErr).Str("hash", hash).Msg("CheckForCompletedTorrents: DB error looking up download")
			continue
		}
		knownDownload := dbErr == nil

		if knownDownload {
			// Known download — only process if not already marked completed.
			if download.Status == models.DownloadStatusCompleted {
				continue
			}

			// Move files first, then mark complete.
			for _, file := range files {
				var moveErr error
				if download.MediaType == "movie" {
					movie, dbErr := db.NewMovieRepository().GetByID(download.MediaID)
					if dbErr != nil {
						log.Warn().Err(dbErr).Str("mediaID", download.MediaID).Msg("CheckForCompletedTorrents: could not load movie for structured move, falling back")
						moveErr = h.fileMover.MoveFile(file, "movie")
					} else {
						_, moveErr = h.fileMover.MoveMovieFile(file, movie)
					}
				} else if download.MediaType == "tv" {
					show, dbErr := db.NewTVShowRepository().GetByID(download.MediaID)
					if dbErr != nil {
						log.Warn().Err(dbErr).Str("mediaID", download.MediaID).Msg("CheckForCompletedTorrents: could not load show for structured move, falling back")
						moveErr = h.fileMover.MoveFile(file, "tvshow")
					} else {
				// Best-effort: find the matching episode by parsing S/E from filename.
					// Fall back to flat move if we cannot identify the episode.
					ep := findEpisodeForFile(file, download.MediaID)
					if ep != nil {
						_, moveErr = h.fileMover.MoveTVFile(file, show, ep)
						if moveErr == nil {
							completedAt := time.Now()
							ep.Status = models.EpisodeStatusDownloaded
							ep.DownloadedAt = &completedAt
							_ = db.NewEpisodeRepository().Save(ep)
						}
					} else {
						moveErr = h.fileMover.MoveFile(file, "tvshow")
					}
					}
				} else {
					moveErr = h.fileMover.MoveFile(file, download.MediaType)
				}
				if moveErr != nil {
					log.Warn().Err(moveErr).Str("file", file).Msg("CheckForCompletedTorrents: failed to move file")
				}
			}

			if err := downloadRepo.UpdateStatus(hash, models.DownloadStatusCompleted); err != nil {
				log.Warn().Err(err).Str("hash", hash).Msg("CheckForCompletedTorrents: failed to update status")
			}

			if h.notifRegistry != nil {
				h.notifRegistry.Fire(notifications.NotificationEvent{
					Type:    "download_complete",
					Title:   "Download Complete",
					Message: fmt.Sprintf("%s has finished downloading.", download.Title),
					User:    download.AddedBy,
				})
			}

			_ = db.NewHistoryRepository().Add(&models.HistoryEntry{
				EventType: models.HistoryEventDownloadComplete,
				Title:     download.Title,
				MediaType: download.MediaType,
				MediaID:   download.MediaID,
				Message:   fmt.Sprintf("Download completed: %s", download.Title),
				User:      download.AddedBy,
			})

			plexScanNeeded = true
			log.Info().Str("hash", hash).Str("title", download.Title).Msg("Download completed")
		} else {
			// Unknown download — fall back to legacy behaviour (move as movie, no Plex scan).
			for _, file := range files {
				if err := h.fileMover.MoveFile(file, "movie"); err != nil {
					log.Warn().Err(err).Str("file", file).Msg("CheckForCompletedTorrents: failed to move file (legacy)")
				}
			}
		}
	}

	if plexScanNeeded && h.mediaServers != nil {
		h.mediaServers.Scan()
	}
}

// findEpisodeForFile attempts to find the Episode record matching a torrent file
// by parsing the S<NN>E<NN> pattern from the filename, then looking up the DB.
func findEpisodeForFile(filePath, showID string) *models.Episode {
	re := regexp.MustCompile(`(?i)S(\d{1,2})E(\d{1,2})`)
	m := re.FindStringSubmatch(filepath.Base(filePath))
	if m == nil {
		return nil
	}
	season, err1 := strconv.Atoi(m[1])
	epNum, err2 := strconv.Atoi(m[2])
	if err1 != nil || err2 != nil {
		return nil
	}
	ep, err := db.NewEpisodeRepository().GetByKey(showID, season, epNum)
	if err != nil {
		return nil
	}
	return ep
}
