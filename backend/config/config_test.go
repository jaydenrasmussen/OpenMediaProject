package config

import (
	"os"
	"testing"
)

// ── Load ──────────────────────────────────────────────────────────────────────

func TestLoad_ReturnsNoError(t *testing.T) {
	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}
	if cfg == nil {
		t.Fatal("Load() returned nil config")
	}
}

func TestLoad_Defaults(t *testing.T) {
	// Clear every env var that Load() reads so defaults kick in.
	keys := []string{
		"SERVER_ADDRESS", "CORS_ALLOW_ORIGINS", "APP_ENV", "LOG_LEVEL",
		"DB_PATH", "API_KEY", "ENABLE_AUTH", "ALLOW_API_KEY_QUERY",
		"SKIP_AUTH_ENDPOINTS", "TVDB_API_KEY", "TMDB_API_KEY",
		"TORRENT_CLIENT_TYPE", "TORRENT_CLIENT_URL",
		"TORRENT_CLIENT_USERNAME", "TORRENT_CLIENT_PASSWORD",
		"RUTORRENT_URL", "RUTORRENT_USERNAME", "RUTORRENT_PASSWORD",
		"PLEX_URL", "PLEX_TOKEN",
		"JELLYFIN_URL", "JELLYFIN_API_KEY",
		"EMBY_URL", "EMBY_API_KEY",
		"MOVER_MOVIE_DIR", "MOVER_TVSHOW_DIR",
		"PREFERRED_QUALITY", "QUALITY_FALLBACK",
		"DISCORD_WEBHOOK_URL", "EMAIL_SMTP_HOST", "EMAIL_SMTP_PORT",
		"EMAIL_USERNAME", "EMAIL_PASSWORD", "EMAIL_FROM", "EMAIL_TO",
		"NTFY_TOPIC_URL", "GOTIFY_URL", "GOTIFY_TOKEN", "APPRISE_URL",
		"INDEXER_URL", "INDEXER_API_KEY", "INDEXER_NAME",
	}
	for _, k := range keys {
		t.Setenv(k, "")
	}

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}

	if cfg.ServerAddress != ":8080" {
		t.Errorf("ServerAddress = %q, want %q", cfg.ServerAddress, ":8080")
	}
	if cfg.CorsAllowOrigins != "*" {
		t.Errorf("CorsAllowOrigins = %q, want %q", cfg.CorsAllowOrigins, "*")
	}
	if cfg.Environment != "development" {
		t.Errorf("Environment = %q, want %q", cfg.Environment, "development")
	}
	if cfg.LogLevel != "info" {
		t.Errorf("LogLevel = %q, want %q", cfg.LogLevel, "info")
	}
	if cfg.Database.DBPath != "./data/nutsdb" {
		t.Errorf("Database.DBPath = %q, want %q", cfg.Database.DBPath, "./data/nutsdb")
	}
	if cfg.Auth.EnableAuth != false {
		t.Errorf("Auth.EnableAuth = %v, want false", cfg.Auth.EnableAuth)
	}
	if cfg.Auth.AllowQueryParam != false {
		t.Errorf("Auth.AllowQueryParam = %v, want false", cfg.Auth.AllowQueryParam)
	}
	if cfg.TorrentClient.ClientType != ClientTypeRuTorrent {
		t.Errorf("TorrentClient.ClientType = %q, want %q", cfg.TorrentClient.ClientType, ClientTypeRuTorrent)
	}
	if cfg.Downloads.PreferredQuality != "1080p" {
		t.Errorf("Downloads.PreferredQuality = %q, want %q", cfg.Downloads.PreferredQuality, "1080p")
	}
	if len(cfg.Downloads.QualityFallback) != 3 {
		t.Errorf("Downloads.QualityFallback len = %d, want 3", len(cfg.Downloads.QualityFallback))
	}
	if cfg.Notifications.EmailSMTPPort != 587 {
		t.Errorf("Notifications.EmailSMTPPort = %d, want 587", cfg.Notifications.EmailSMTPPort)
	}
	if len(cfg.Indexers) != 0 {
		t.Errorf("Indexers len = %d, want 0", len(cfg.Indexers))
	}
}

func TestLoad_EnvOverrides(t *testing.T) {
	t.Setenv("SERVER_ADDRESS", ":9090")
	t.Setenv("APP_ENV", "production")
	t.Setenv("LOG_LEVEL", "debug")
	t.Setenv("API_KEY", "super-secret")
	t.Setenv("ENABLE_AUTH", "true")
	t.Setenv("ALLOW_API_KEY_QUERY", "yes")
	t.Setenv("TVDB_API_KEY", "tvdb-abc")
	t.Setenv("TMDB_API_KEY", "tmdb-xyz")
	t.Setenv("TORRENT_CLIENT_TYPE", "transmission")
	t.Setenv("TORRENT_CLIENT_URL", "http://localhost:9091")
	t.Setenv("PREFERRED_QUALITY", "2160p")
	t.Setenv("PLEX_URL", "http://plex:32400")
	t.Setenv("PLEX_TOKEN", "my-plex-token")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}

	if cfg.ServerAddress != ":9090" {
		t.Errorf("ServerAddress = %q, want %q", cfg.ServerAddress, ":9090")
	}
	if cfg.Environment != "production" {
		t.Errorf("Environment = %q, want %q", cfg.Environment, "production")
	}
	if cfg.LogLevel != "debug" {
		t.Errorf("LogLevel = %q, want %q", cfg.LogLevel, "debug")
	}
	if cfg.Auth.APIKey != "super-secret" {
		t.Errorf("Auth.APIKey = %q, want %q", cfg.Auth.APIKey, "super-secret")
	}
	if !cfg.Auth.EnableAuth {
		t.Errorf("Auth.EnableAuth = false, want true")
	}
	if !cfg.Auth.AllowQueryParam {
		t.Errorf("Auth.AllowQueryParam = false, want true")
	}
	if cfg.TVDB.APIKey != "tvdb-abc" {
		t.Errorf("TVDB.APIKey = %q, want %q", cfg.TVDB.APIKey, "tvdb-abc")
	}
	if cfg.TMDB.APIKey != "tmdb-xyz" {
		t.Errorf("TMDB.APIKey = %q, want %q", cfg.TMDB.APIKey, "tmdb-xyz")
	}
	if cfg.TorrentClient.ClientType != ClientTypeTransmission {
		t.Errorf("TorrentClient.ClientType = %q, want %q", cfg.TorrentClient.ClientType, ClientTypeTransmission)
	}
	if cfg.TorrentClient.BaseURL != "http://localhost:9091" {
		t.Errorf("TorrentClient.BaseURL = %q, want %q", cfg.TorrentClient.BaseURL, "http://localhost:9091")
	}
	if cfg.Downloads.PreferredQuality != "2160p" {
		t.Errorf("Downloads.PreferredQuality = %q, want %q", cfg.Downloads.PreferredQuality, "2160p")
	}
	if cfg.Plex.URL != "http://plex:32400" {
		t.Errorf("Plex.URL = %q, want %q", cfg.Plex.URL, "http://plex:32400")
	}
}

func TestLoad_IndexerFromEnv(t *testing.T) {
	t.Setenv("INDEXER_URL", "http://prowlarr:9696")
	t.Setenv("INDEXER_API_KEY", "prowlarr-api-key")
	t.Setenv("INDEXER_NAME", "MyProwlarr")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}

	if len(cfg.Indexers) != 1 {
		t.Fatalf("Indexers len = %d, want 1", len(cfg.Indexers))
	}
	if cfg.Indexers[0].URL != "http://prowlarr:9696" {
		t.Errorf("Indexers[0].URL = %q, want %q", cfg.Indexers[0].URL, "http://prowlarr:9696")
	}
	if cfg.Indexers[0].APIKey != "prowlarr-api-key" {
		t.Errorf("Indexers[0].APIKey = %q, want %q", cfg.Indexers[0].APIKey, "prowlarr-api-key")
	}
	if cfg.Indexers[0].Name != "MyProwlarr" {
		t.Errorf("Indexers[0].Name = %q, want %q", cfg.Indexers[0].Name, "MyProwlarr")
	}
}

func TestLoad_IndexerDefaultName(t *testing.T) {
	t.Setenv("INDEXER_URL", "http://prowlarr:9696")
	// INDEXER_NAME not set → defaults to "Prowlarr"
	t.Setenv("INDEXER_NAME", "")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}
	if len(cfg.Indexers) != 1 {
		t.Fatalf("Indexers len = %d, want 1", len(cfg.Indexers))
	}
	if cfg.Indexers[0].Name != "Prowlarr" {
		t.Errorf("Indexers[0].Name = %q, want %q", cfg.Indexers[0].Name, "Prowlarr")
	}
}

func TestLoad_SkipAuthEndpointsCustom(t *testing.T) {
	t.Setenv("SKIP_AUTH_ENDPOINTS", "/health,/ping,/status")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}
	if len(cfg.Auth.SkipAuthEndpoints) != 3 {
		t.Errorf("SkipAuthEndpoints len = %d, want 3", len(cfg.Auth.SkipAuthEndpoints))
	}
}

func TestLoad_NotificationsFromEnv(t *testing.T) {
	t.Setenv("DISCORD_WEBHOOK_URL", "https://discord.com/webhooks/123")
	t.Setenv("EMAIL_SMTP_PORT", "465")
	t.Setenv("NTFY_TOPIC_URL", "https://ntfy.sh/my-topic")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}
	if cfg.Notifications.DiscordWebhookURL != "https://discord.com/webhooks/123" {
		t.Errorf("DiscordWebhookURL = %q", cfg.Notifications.DiscordWebhookURL)
	}
	if cfg.Notifications.EmailSMTPPort != 465 {
		t.Errorf("EmailSMTPPort = %d, want 465", cfg.Notifications.EmailSMTPPort)
	}
	if cfg.Notifications.NtfyTopicURL != "https://ntfy.sh/my-topic" {
		t.Errorf("NtfyTopicURL = %q", cfg.Notifications.NtfyTopicURL)
	}
}

// ── getEnvBool ────────────────────────────────────────────────────────────────

func TestGetEnvBool_TrueVariants(t *testing.T) {
	const key = "_TEST_BOOL_OPENMEDIA_UNIQUE"
	for _, val := range []string{"true", "TRUE", "True", "1", "yes", "YES", "y", "Y"} {
		t.Setenv(key, val)
		if !getEnvBool(key, false) {
			t.Errorf("getEnvBool(%q) = false, want true for value %q", key, val)
		}
	}
}

func TestGetEnvBool_FalseVariants(t *testing.T) {
	const key = "_TEST_BOOL_OPENMEDIA_UNIQUE"
	for _, val := range []string{"false", "FALSE", "0", "no", "NO", "n", "N", "random"} {
		t.Setenv(key, val)
		if getEnvBool(key, true) {
			t.Errorf("getEnvBool(%q) = true, want false for value %q", key, val)
		}
	}
}

func TestGetEnvBool_EmptyUsesDefault(t *testing.T) {
	const key = "_TEST_BOOL_OPENMEDIA_UNIQUE"
	os.Unsetenv(key)

	if getEnvBool(key, true) != true {
		t.Errorf("getEnvBool should return default true when key is unset")
	}
	if getEnvBool(key, false) != false {
		t.Errorf("getEnvBool should return default false when key is unset")
	}
}

// ── getEnvInt ─────────────────────────────────────────────────────────────────

func TestGetEnvInt_ValidInteger(t *testing.T) {
	const key = "_TEST_INT_OPENMEDIA_UNIQUE"
	tests := []struct {
		val  string
		want int
	}{
		{"587", 587},
		{"0", 0},
		{"9999", 9999},
		{"  42  ", 42}, // leading/trailing spaces trimmed
	}
	for _, tt := range tests {
		t.Setenv(key, tt.val)
		got := getEnvInt(key, 0)
		if got != tt.want {
			t.Errorf("getEnvInt(%q) (value=%q) = %d, want %d", key, tt.val, got, tt.want)
		}
	}
}

func TestGetEnvInt_InvalidUsesDefault(t *testing.T) {
	const key = "_TEST_INT_OPENMEDIA_UNIQUE"
	t.Setenv(key, "notanint")
	if got := getEnvInt(key, 42); got != 42 {
		t.Errorf("getEnvInt with invalid value = %d, want default 42", got)
	}
}

func TestGetEnvInt_EmptyUsesDefault(t *testing.T) {
	const key = "_TEST_INT_OPENMEDIA_UNIQUE"
	os.Unsetenv(key)
	if got := getEnvInt(key, 99); got != 99 {
		t.Errorf("getEnvInt with unset key = %d, want default 99", got)
	}
}

// ── getEnvStringSlice ─────────────────────────────────────────────────────────

func TestGetEnvStringSlice_CommaSeparated(t *testing.T) {
	const key = "_TEST_SLICE_OPENMEDIA_UNIQUE"
	t.Setenv(key, "a,b,c")
	got := getEnvStringSlice(key, nil)
	if len(got) != 3 || got[0] != "a" || got[1] != "b" || got[2] != "c" {
		t.Errorf("got %v, want [a b c]", got)
	}
}

func TestGetEnvStringSlice_TrimsSpaces(t *testing.T) {
	const key = "_TEST_SLICE_OPENMEDIA_UNIQUE"
	t.Setenv(key, " a , b , c ")
	got := getEnvStringSlice(key, nil)
	if len(got) != 3 || got[0] != "a" || got[1] != "b" || got[2] != "c" {
		t.Errorf("got %v, want trimmed [a b c]", got)
	}
}

func TestGetEnvStringSlice_SkipsEmptyParts(t *testing.T) {
	const key = "_TEST_SLICE_OPENMEDIA_UNIQUE"
	t.Setenv(key, "a,,b,,c")
	got := getEnvStringSlice(key, nil)
	if len(got) != 3 {
		t.Errorf("got len=%d, want 3 (empty parts skipped): %v", len(got), got)
	}
}

func TestGetEnvStringSlice_SingleValue(t *testing.T) {
	const key = "_TEST_SLICE_OPENMEDIA_UNIQUE"
	t.Setenv(key, "only")
	got := getEnvStringSlice(key, nil)
	if len(got) != 1 || got[0] != "only" {
		t.Errorf("got %v, want [only]", got)
	}
}

func TestGetEnvStringSlice_EmptyUsesDefault(t *testing.T) {
	const key = "_TEST_SLICE_OPENMEDIA_UNIQUE"
	os.Unsetenv(key)
	def := []string{"x", "y", "z"}
	got := getEnvStringSlice(key, def)
	if len(got) != 3 || got[0] != "x" {
		t.Errorf("got %v, want default %v", got, def)
	}
}
