package config

import (
	"fmt"
	"os"
	"strings"

	"backend/db"
)

// DownloadPreferences holds quality and behaviour preferences for automatic downloads
type DownloadPreferences struct {
	PreferredQuality string   // "2160p", "1080p", "720p", "480p", "any" — default "1080p"
	QualityFallback  []string // ordered fallback list — default ["1080p","720p","any"]
}

// IndexerConfig holds configuration for a single indexer (env-based quick setup)
type IndexerConfig struct {
	URL    string
	APIKey string
	Name   string
}

// Config holds all configuration for our application
type Config struct {
	ServerAddress    string
	CorsAllowOrigins string
	Database         db.DatabaseConfig
	Environment      string
	LogLevel         string
	Auth             AuthConfig
	TVDB             TVDBConfig
	TMDB             TMDBConfig
	TorrentClient    TorrentClientConfig
	RuTorrent        RuTorrentConfig // For backward compatibility
	Plex             PlexConfig
	Jellyfin         JellyfinConfig
	Emby             EmbyConfig
	FileMover        FileMoverConfig
	Indexers         []IndexerConfig
	Downloads        DownloadPreferences
	Notifications    NotificationsConfig
}

// FileMoverConfig holds file mover related configuration
type FileMoverConfig struct {
	MovieDir  string
	TVShowDir string
}

// PlexConfig holds Plex related configuration
type PlexConfig struct {
	URL   string
	Token string
}

// JellyfinConfig holds Jellyfin related configuration
type JellyfinConfig struct {
	URL    string
	APIKey string
}

// EmbyConfig holds Emby related configuration
type EmbyConfig struct {
	URL    string
	APIKey string
}

// TVDBConfig holds TVDB API related configuration
type TVDBConfig struct {
	APIKey string
}

// TMDBConfig holds TMDB API related configuration
type TMDBConfig struct {
	APIKey string
}

// AuthConfig holds authentication related configuration
type AuthConfig struct {
	APIKey            string
	EnableAuth        bool
	AllowQueryParam   bool
	SkipAuthEndpoints []string
}

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

// TorrentClientConfig holds configuration for the torrent client
type TorrentClientConfig struct {
	ClientType TorrentClientType `json:"client_type"`
	BaseURL    string            `json:"base_url"`
	Username   string            `json:"username"`
	Password   string            `json:"password"`
}

// NotificationsConfig holds configuration for all notification backends
type NotificationsConfig struct {
	DiscordWebhookURL string
	EmailSMTPHost     string
	EmailSMTPPort     int
	EmailUsername     string
	EmailPassword     string
	EmailFrom         string
	EmailTo           []string
	NtfyTopicURL      string
	GotifyURL         string
	GotifyToken       string
	AppriseURL        string
}

// RuTorrentConfig holds ruTorrent API related configuration (legacy)
type RuTorrentConfig struct {
	BaseURL  string
	Username string
	Password string
}

// Load returns the application configuration from environment variables
func Load() (*Config, error) {
	// Default configuration values
	cfg := &Config{
		ServerAddress:    getEnv("SERVER_ADDRESS", ":8080"),
		CorsAllowOrigins: getEnv("CORS_ALLOW_ORIGINS", "*"),
		Environment:      getEnv("APP_ENV", "development"),
		LogLevel:         getEnv("LOG_LEVEL", "info"),
		Database: db.DatabaseConfig{
			DBPath: getEnv("DB_PATH", "./data/nutsdb"),
		},
		Auth: AuthConfig{
			APIKey:            getEnv("API_KEY", ""),
			EnableAuth:        getEnvBool("ENABLE_AUTH", false),
			AllowQueryParam:   getEnvBool("ALLOW_API_KEY_QUERY", false),
			SkipAuthEndpoints: getEnvStringSlice("SKIP_AUTH_ENDPOINTS", []string{"/api/health", "/api/v1/health", "/api/v1/auth"}),
		},
		TVDB: TVDBConfig{
			APIKey: getEnv("TVDB_API_KEY", ""),
		},
		TMDB: TMDBConfig{
			APIKey: getEnv("TMDB_API_KEY", ""),
		},
		TorrentClient: TorrentClientConfig{
			ClientType: TorrentClientType(getEnv("TORRENT_CLIENT_TYPE", string(ClientTypeRuTorrent))),
			BaseURL:    getEnv("TORRENT_CLIENT_URL", ""),
			Username:   getEnv("TORRENT_CLIENT_USERNAME", ""),
			Password:   getEnv("TORRENT_CLIENT_PASSWORD", ""),
		},
		// For backward compatibility
		RuTorrent: RuTorrentConfig{
			BaseURL:  getEnv("RUTORRENT_URL", ""),
			Username: getEnv("RUTORRENT_USERNAME", ""),
			Password: getEnv("RUTORRENT_PASSWORD", ""),
		},
		Plex: PlexConfig{
			URL:   getEnv("PLEX_URL", ""),
			Token: getEnv("PLEX_TOKEN", ""),
		},
		Jellyfin: JellyfinConfig{
			URL:    getEnv("JELLYFIN_URL", ""),
			APIKey: getEnv("JELLYFIN_API_KEY", ""),
		},
		Emby: EmbyConfig{
			URL:    getEnv("EMBY_URL", ""),
			APIKey: getEnv("EMBY_API_KEY", ""),
		},
		FileMover: FileMoverConfig{
			MovieDir:  getEnv("MOVER_MOVIE_DIR", ""),
			TVShowDir: getEnv("MOVER_TVSHOW_DIR", ""),
		},
		Downloads: DownloadPreferences{
			PreferredQuality: getEnv("PREFERRED_QUALITY", "1080p"),
			QualityFallback:  getEnvStringSlice("QUALITY_FALLBACK", []string{"1080p", "720p", "any"}),
		},
		Notifications: NotificationsConfig{
			DiscordWebhookURL: getEnv("DISCORD_WEBHOOK_URL", ""),
			EmailSMTPHost:     getEnv("EMAIL_SMTP_HOST", ""),
			EmailSMTPPort:     getEnvInt("EMAIL_SMTP_PORT", 587),
			EmailUsername:     getEnv("EMAIL_USERNAME", ""),
			EmailPassword:     getEnv("EMAIL_PASSWORD", ""),
			EmailFrom:         getEnv("EMAIL_FROM", ""),
			EmailTo:           getEnvStringSlice("EMAIL_TO", []string{}),
			NtfyTopicURL:      getEnv("NTFY_TOPIC_URL", ""),
			GotifyURL:         getEnv("GOTIFY_URL", ""),
			GotifyToken:       getEnv("GOTIFY_TOKEN", ""),
			AppriseURL:        getEnv("APPRISE_URL", ""),
		},
	}

	// Populate indexers from env vars (single indexer bootstrap)
	if indexerURL := getEnv("INDEXER_URL", ""); indexerURL != "" {
		cfg.Indexers = append(cfg.Indexers, IndexerConfig{
			URL:    indexerURL,
			APIKey: getEnv("INDEXER_API_KEY", ""),
			Name:   getEnv("INDEXER_NAME", "Prowlarr"),
		})
	}

	return cfg, nil
}

// Helper function to read environment variables with a default value
func getEnv(key, defaultValue string) string {
	value := os.Getenv(key)
	if value == "" {
		return defaultValue
	}
	return strings.TrimSpace(value)
}

// Helper function to read boolean environment variables
func getEnvBool(key string, defaultValue bool) bool {
	value := os.Getenv(key)
	if value == "" {
		return defaultValue
	}

	value = strings.ToLower(strings.TrimSpace(value))
	return value == "true" || value == "1" || value == "yes" || value == "y"
}

// Helper function to read integer environment variables
func getEnvInt(key string, defaultValue int) int {
	value := os.Getenv(key)
	if value == "" {
		return defaultValue
	}
	value = strings.TrimSpace(value)
	var result int
	if _, err := fmt.Sscanf(value, "%d", &result); err != nil {
		return defaultValue
	}
	return result
}

// Helper function to read string slice environment variables
func getEnvStringSlice(key string, defaultValue []string) []string {
	value := os.Getenv(key)
	if value == "" {
		return defaultValue
	}

	parts := strings.Split(value, ",")
	var result []string
	for _, part := range parts {
		trimmed := strings.TrimSpace(part)
		if trimmed != "" {
			result = append(result, trimmed)
		}
	}

	return result
}
