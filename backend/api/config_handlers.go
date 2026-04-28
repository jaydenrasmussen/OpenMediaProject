package api

import (
	"fmt"
	"os"
	"strings"
	"time"

	"backend/clients"
	"backend/config"

	"github.com/gofiber/fiber/v2"
	"github.com/rs/zerolog/log"
)

// ConfigResponse represents the current system configuration
type ConfigResponse struct {
	ServerConfig    ServerConfigResponse   `json:"server"`
	AuthConfig      AuthConfigResponse     `json:"auth"`
	DatabaseConfig  DatabaseConfigResponse `json:"database"`
	TorrentConfig   TorrentConfigResponse  `json:"torrent_client"`
	TVDBConfig      APIKeyConfigResponse   `json:"tvdb"`
	TMDBConfig      APIKeyConfigResponse   `json:"tmdb"`
	SystemInfo      SystemInfoResponse     `json:"system_info"`
	AllowedSettings []string               `json:"allowed_settings"`
}

// ServerConfigResponse represents server configuration
type ServerConfigResponse struct {
	Address          string `json:"address"`
	CorsAllowOrigins string `json:"cors_allow_origins"`
	Environment      string `json:"environment"`
	LogLevel         string `json:"log_level"`
}

// AuthConfigResponse represents authentication configuration
type AuthConfigResponse struct {
	Enabled         bool     `json:"enabled"`
	AllowQueryParam bool     `json:"allow_query_param"`
	SkipEndpoints   []string `json:"skip_endpoints"`
	// API key is intentionally not included for security reasons
}

// DatabaseConfigResponse represents database configuration
type DatabaseConfigResponse struct {
	Path string `json:"path"`
}

// TorrentConfigResponse represents torrent client configuration
type TorrentConfigResponse struct {
	ClientType string `json:"client_type"`
	URL        string `json:"url"`
	Username   string `json:"username"`
	// Password is intentionally not included for security reasons
	Connected bool   `json:"connected"`
	Message   string `json:"message,omitempty"`
}

// APIKeyConfigResponse represents API key configuration
type APIKeyConfigResponse struct {
	Configured bool `json:"configured"`
	// The actual API key is intentionally not included for security reasons
}

// SystemInfoResponse represents system information
type SystemInfoResponse struct {
	Version      string    `json:"version"`
	StartTime    time.Time `json:"start_time"`
	Uptime       string    `json:"uptime"`
	GoVersion    string    `json:"go_version"`
	OSArch       string    `json:"os_arch"`
	NumGoroutine int       `json:"num_goroutine"`
}

// ConfigUpdateRequest represents a request to update configuration
type ConfigUpdateRequest struct {
	TorrentClient *TorrentClientUpdateRequest `json:"torrent_client,omitempty"`
	TVDB          *APIKeyUpdateRequest        `json:"tvdb,omitempty"`
	TMDB          *APIKeyUpdateRequest        `json:"tmdb,omitempty"`
	Server        *ServerUpdateRequest        `json:"server,omitempty"`
}

// TorrentClientUpdateRequest represents a request to update torrent client configuration
type TorrentClientUpdateRequest struct {
	ClientType string `json:"client_type"`
	URL        string `json:"url"`
	Username   string `json:"username"`
	Password   string `json:"password"`
}

// APIKeyUpdateRequest represents a request to update an API key
type APIKeyUpdateRequest struct {
	APIKey string `json:"api_key"`
}

// ServerUpdateRequest represents a request to update server configuration
type ServerUpdateRequest struct {
	CorsAllowOrigins string `json:"cors_allow_origins"`
	LogLevel         string `json:"log_level"`
}

// startTime is the time when the server started
var startTime = time.Now()

// GetSystemConfig returns the current system configuration
func GetSystemConfig(c *fiber.Ctx) error {
	// Load configuration
	cfg, err := config.Load()
	if err != nil {
		log.Error().Err(err).Msg("Failed to load configuration")
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to load configuration: " + err.Error(),
		})
	}

	log.Debug().Msg("Preparing system configuration response")
	// Prepare the response
	resp := ConfigResponse{
		ServerConfig: ServerConfigResponse{
			Address:          cfg.ServerAddress,
			CorsAllowOrigins: cfg.CorsAllowOrigins,
			Environment:      cfg.Environment,
			LogLevel:         cfg.LogLevel,
		},
		AuthConfig: AuthConfigResponse{
			Enabled:         cfg.Auth.EnableAuth,
			AllowQueryParam: cfg.Auth.AllowQueryParam,
			SkipEndpoints:   cfg.Auth.SkipAuthEndpoints,
		},
		DatabaseConfig: DatabaseConfigResponse{
			Path: cfg.Database.DBPath,
		},
		TorrentConfig: TorrentConfigResponse{
			ClientType: string(cfg.TorrentClient.ClientType),
			URL:        cfg.TorrentClient.BaseURL,
			Username:   cfg.TorrentClient.Username,
			// Password is intentionally not included
		},
		TVDBConfig: APIKeyConfigResponse{
			Configured: cfg.TVDB.APIKey != "",
		},
		TMDBConfig: APIKeyConfigResponse{
			Configured: cfg.TMDB.APIKey != "",
		},
		SystemInfo: SystemInfoResponse{
			Version:      "1.0.0", // Replace with actual version
			StartTime:    startTime,
			Uptime:       formatUptime(time.Since(startTime)),
			GoVersion:    getGoVersion(),
			OSArch:       fmt.Sprintf("%s/%s", getOS(), getArch()),
			NumGoroutine: getNumGoroutine(),
		},
		AllowedSettings: []string{
			"torrent_client", "tvdb", "tmdb", "server",
		},
	}

	// Check if torrent client is configured and connected
	if cfg.TorrentClient.BaseURL != "" {
		log.Debug().Str("client_type", string(cfg.TorrentClient.ClientType)).Str("url", cfg.TorrentClient.BaseURL).Msg("Checking torrent client connection")
		clientConfig := clients.TorrentClientConfig{
			ClientType: clients.TorrentClientType(cfg.TorrentClient.ClientType),
			BaseURL:    cfg.TorrentClient.BaseURL,
			Username:   cfg.TorrentClient.Username,
			Password:   cfg.TorrentClient.Password,
		}

		client, err := clients.NewTorrentClient(clientConfig)
		if err == nil {
			connected, message := client.CheckHealth()
			resp.TorrentConfig.Connected = connected
			resp.TorrentConfig.Message = message
			log.Debug().Bool("connected", connected).Str("message", message).Msg("Torrent client connection check completed")
		} else {
			log.Error().Err(err).Msg("Failed to create torrent client for health check")
		}
	}

	return c.JSON(resp)
}

// UpdateSystemConfig updates the system configuration
func UpdateSystemConfig(c *fiber.Ctx) error {
	// Parse request body
	var req ConfigUpdateRequest
	if err := c.BodyParser(&req); err != nil {
		log.Error().Err(err).Msg("Failed to parse configuration update request body")
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Invalid request body: " + err.Error(),
		})
	}
	log.Debug().Interface("request", req).Msg("Processing configuration update request")

	// Load current configuration
	cfg, err := config.Load()
	if err != nil {
		log.Error().Err(err).Msg("Failed to load configuration for update")
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to load configuration: " + err.Error(),
		})
	}

	// Update torrent client configuration if provided
	if req.TorrentClient != nil {
		log.Info().Interface("torrent_client", req.TorrentClient).Msg("Updating torrent client configuration")
		// Validate client type
		clientType := strings.ToLower(req.TorrentClient.ClientType)
		if clientType != string(config.ClientTypeRuTorrent) &&
			clientType != string(config.ClientTypeTransmission) &&
			clientType != string(config.ClientTypeDeluge) {
			return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
				"error":   true,
				"message": "Invalid torrent client type. Must be one of: rutorrent, transmission, deluge",
			})
		}

		// Validate URL
		if req.TorrentClient.URL == "" {
			log.Warn().Msg("Empty torrent client URL provided")
			return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
				"error":   true,
				"message": "Torrent client URL is required",
			})
		}

		// Update configuration
		cfg.TorrentClient.ClientType = config.TorrentClientType(clientType)
		cfg.TorrentClient.BaseURL = req.TorrentClient.URL
		cfg.TorrentClient.Username = req.TorrentClient.Username
		cfg.TorrentClient.Password = req.TorrentClient.Password

		// Update environment variables
		os.Setenv("TORRENT_CLIENT_TYPE", clientType)
		os.Setenv("TORRENT_CLIENT_URL", req.TorrentClient.URL)
		os.Setenv("TORRENT_CLIENT_USERNAME", req.TorrentClient.Username)
		os.Setenv("TORRENT_CLIENT_PASSWORD", req.TorrentClient.Password)

		log.Info().
			Str("client_type", clientType).
			Str("url", req.TorrentClient.URL).
			Str("username", req.TorrentClient.Username).
			Msg("Updated torrent client environment variables")

		// Test connection
		clientConfig := clients.TorrentClientConfig{
			ClientType: clients.TorrentClientType(cfg.TorrentClient.ClientType),
			BaseURL:    cfg.TorrentClient.BaseURL,
			Username:   cfg.TorrentClient.Username,
			Password:   cfg.TorrentClient.Password,
		}

		client, err := clients.NewTorrentClient(clientConfig)
		if err != nil {
			log.Error().Err(err).Msg("Failed to create torrent client")
			return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
				"error":   true,
				"message": "Failed to create torrent client: " + err.Error(),
				"status":  "not_connected",
			})
		}

		connected, message := client.CheckHealth()
		if !connected {
			log.Error().Str("message", message).Msg("Failed to connect to torrent client")
			return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
				"error":   true,
				"message": "Failed to connect to torrent client: " + message,
				"status":  "not_connected",
			})
		}
		log.Info().Str("message", message).Msg("Successfully connected to torrent client")
	}

	// Update TVDB API key if provided
	if req.TVDB != nil && req.TVDB.APIKey != "" {
		cfg.TVDB.APIKey = req.TVDB.APIKey
		os.Setenv("TVDB_API_KEY", req.TVDB.APIKey)
		log.Info().Msg("Updated TVDB API key")
	}

	// Update TMDB API key if provided
	if req.TMDB != nil && req.TMDB.APIKey != "" {
		cfg.TMDB.APIKey = req.TMDB.APIKey
		os.Setenv("TMDB_API_KEY", req.TMDB.APIKey)
		log.Info().Msg("Updated TMDB API key")
	}

	// Update server configuration if provided
	if req.Server != nil {
		log.Info().Interface("server_config", req.Server).Msg("Updating server configuration")
		if req.Server.CorsAllowOrigins != "" {
			cfg.CorsAllowOrigins = req.Server.CorsAllowOrigins
			os.Setenv("CORS_ALLOW_ORIGINS", req.Server.CorsAllowOrigins)
			log.Debug().Str("cors_allow_origins", req.Server.CorsAllowOrigins).Msg("Updated CORS settings")
		}

		if req.Server.LogLevel != "" {
			cfg.LogLevel = req.Server.LogLevel
			os.Setenv("LOG_LEVEL", req.Server.LogLevel)
			log.Debug().Str("log_level", req.Server.LogLevel).Msg("Updated log level")
		}
	}

	log.Info().Msg("Configuration updated successfully")
	return c.JSON(fiber.Map{
		"success": true,
		"message": "Configuration updated successfully",
		"config": ConfigResponse{
			ServerConfig: ServerConfigResponse{
				Address:          cfg.ServerAddress,
				CorsAllowOrigins: cfg.CorsAllowOrigins,
				Environment:      cfg.Environment,
				LogLevel:         cfg.LogLevel,
			},
			AuthConfig: AuthConfigResponse{
				Enabled:         cfg.Auth.EnableAuth,
				AllowQueryParam: cfg.Auth.AllowQueryParam,
				SkipEndpoints:   cfg.Auth.SkipAuthEndpoints,
			},
			DatabaseConfig: DatabaseConfigResponse{
				Path: cfg.Database.DBPath,
			},
			TorrentConfig: TorrentConfigResponse{
				ClientType: string(cfg.TorrentClient.ClientType),
				URL:        cfg.TorrentClient.BaseURL,
				Username:   cfg.TorrentClient.Username,
				Connected:  true,
			},
			TVDBConfig: APIKeyConfigResponse{
				Configured: cfg.TVDB.APIKey != "",
			},
			TMDBConfig: APIKeyConfigResponse{
				Configured: cfg.TMDB.APIKey != "",
			},
		},
	})
}

// TestTorrentConnection tests the connection to a torrent client
func TestTorrentConnection(c *fiber.Ctx) error {
	// Parse request body
	var req TorrentClientUpdateRequest
	if err := c.BodyParser(&req); err != nil {
		log.Error().Err(err).Msg("Failed to parse torrent client test request")
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Invalid request body: " + err.Error(),
		})
	}
	log.Debug().Interface("request", req).Msg("Testing torrent client connection")

	// Validate client type
	clientType := strings.ToLower(req.ClientType)
	if clientType != string(config.ClientTypeRuTorrent) &&
		clientType != string(config.ClientTypeTransmission) &&
		clientType != string(config.ClientTypeDeluge) {
		log.Warn().Str("client_type", clientType).Msg("Invalid torrent client type in test request")
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Invalid torrent client type. Must be one of: rutorrent, transmission, deluge",
		})
	}

	// Validate URL
	if req.URL == "" {
		log.Warn().Msg("Empty URL in torrent client test request")
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Torrent client URL is required",
		})
	}

	// Test connection
	clientConfig := clients.TorrentClientConfig{
		ClientType: clients.TorrentClientType(clientType),
		BaseURL:    req.URL,
		Username:   req.Username,
		Password:   req.Password,
	}

	client, err := clients.NewTorrentClient(clientConfig)
	if err != nil {
		log.Error().Err(err).
			Str("client_type", clientType).
			Str("url", req.URL).
			Msg("Failed to create torrent client for test")
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"connected": false,
			"message":   "Failed to create torrent client: " + err.Error(),
		})
	}

	connected, message := client.CheckHealth()
	log.Info().
		Bool("connected", connected).
		Str("message", message).
		Str("client_type", clientType).
		Str("url", req.URL).
		Msg("Torrent client connection test result")

	// Get client info if connected
	var clientInfo *clients.TorrentClientInfo
	if connected {
		clientInfo, err = client.GetClientInfo()
		if err != nil {
			log.Warn().Err(err).Msg("Failed to get torrent client info after successful connection")
		} else {
			log.Debug().Interface("client_info", clientInfo).Msg("Retrieved torrent client info")
		}
	}

	return c.JSON(fiber.Map{
		"connected":   connected,
		"message":     message,
		"client_info": clientInfo,
	})
}

// Helper functions for system information
func formatUptime(uptime time.Duration) string {
	days := int(uptime.Hours() / 24)
	hours := int(uptime.Hours()) % 24
	minutes := int(uptime.Minutes()) % 60
	seconds := int(uptime.Seconds()) % 60

	if days > 0 {
		return fmt.Sprintf("%dd %dh %dm %ds", days, hours, minutes, seconds)
	} else if hours > 0 {
		return fmt.Sprintf("%dh %dm %ds", hours, minutes, seconds)
	} else if minutes > 0 {
		return fmt.Sprintf("%dm %ds", minutes, seconds)
	}
	return fmt.Sprintf("%ds", seconds)
}

func getGoVersion() string {
	return "go1.21" // This would typically use runtime.Version() but simplified for this example
}

func getOS() string {
	return "linux" // This would typically use runtime.GOOS but simplified for this example
}

func getArch() string {
	return "amd64" // This would typically use runtime.GOARCH but simplified for this example
}

func getNumGoroutine() int {
	return 10 // This would typically use runtime.NumGoroutine() but simplified for this example
}
