package api

import (
	"strconv"
	"strings"

	"backend/clients/notifications"
	"backend/config"
	"backend/db"

	"github.com/gofiber/fiber/v2"
)

// SettingsHandler handles persistent application settings (stored in NutsDB).
type SettingsHandler struct {
	cfg          *config.Config
	notifRegistry *notifications.Registry
}

// NewSettingsHandler creates a new SettingsHandler.
func NewSettingsHandler(cfg *config.Config, notifRegistry *notifications.Registry) *SettingsHandler {
	return &SettingsHandler{cfg: cfg, notifRegistry: notifRegistry}
}

// GetSettings handles GET /api/v1/settings.
// Returns persisted settings merged with current env-derived defaults so the
// response always has a complete picture of the running configuration.
func (h *SettingsHandler) GetSettings(c *fiber.Ctx) error {
	repo := db.NewConfigRepository()
	stored, err := repo.GetSettings()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to load settings: " + err.Error(),
		})
	}

	// Merge: env defaults first, then any persisted overrides on top.
	merged := h.envDefaults()
	for k, v := range stored {
		merged[k] = v
	}

	return c.JSON(merged)
}

// UpdateSettings handles PUT /api/v1/settings.
// Accepts a partial or full settings map and persists it.
func (h *SettingsHandler) UpdateSettings(c *fiber.Ctx) error {
	var body map[string]interface{}
	if err := c.BodyParser(&body); err != nil {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Invalid request body: " + err.Error(),
		})
	}

	repo := db.NewConfigRepository()
	// Load existing to merge (so callers can send partial updates).
	existing, err := repo.GetSettings()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to load existing settings: " + err.Error(),
		})
	}
	for k, v := range body {
		existing[k] = v
	}

	if err := repo.SaveSettings(existing); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to save settings: " + err.Error(),
		})
	}

	// Apply changes to the live config so they take effect immediately.
	applyToCfg(existing, h.cfg)

	// If any indexer field was changed, sync cfg.Indexers[0] back to the DB
	// so the search clients pick up the new URL/key without a restart.
	indexerChanged := false
	for _, k := range []string{"indexer_url", "indexer_api_key", "indexer_name"} {
		if _, ok := body[k]; ok {
			indexerChanged = true
			break
		}
	}
	if indexerChanged {
		syncIndexerToDB(h.cfg)
	}

	// Rebuild the notifier registry so notification settings take effect
	// without requiring a restart.
	RebuildNotifRegistry(h.cfg, h.notifRegistry)

	return c.JSON(fiber.Map{
		"success": true,
		"message": "Settings saved",
	})
}

// envDefaults builds a settings map from the currently-loaded env config so the
// UI always has sensible starting values even before anything is persisted.
// Sensitive credentials are included so the UI can show what is currently active.
func (h *SettingsHandler) envDefaults() map[string]interface{} {
	cfg := h.cfg

	// First indexer (env-bootstrapped single-indexer setup).
	var indexerURL, indexerAPIKey, indexerName string
	if len(cfg.Indexers) > 0 {
		indexerURL = cfg.Indexers[0].URL
		indexerAPIKey = cfg.Indexers[0].APIKey
		indexerName = cfg.Indexers[0].Name
	}

	return map[string]interface{}{
		// Metadata API keys
		"tmdb_api_key": cfg.TMDB.APIKey,
		"tvdb_api_key": cfg.TVDB.APIKey,
		// Torrent client
		"torrent_client_type":     string(cfg.TorrentClient.ClientType),
		"torrent_client_url":      cfg.TorrentClient.BaseURL,
		"torrent_client_username": cfg.TorrentClient.Username,
		"torrent_client_password": cfg.TorrentClient.Password,
		// Library paths
		"movie_dir":  cfg.FileMover.MovieDir,
		"tvshow_dir": cfg.FileMover.TVShowDir,
		// Quality
		"preferred_quality": cfg.Downloads.PreferredQuality,
		"quality_fallback":  strings.Join(cfg.Downloads.QualityFallback, ","),
		// Media servers
		"plex_url":         cfg.Plex.URL,
		"plex_token":       cfg.Plex.Token,
		"jellyfin_url":     cfg.Jellyfin.URL,
		"jellyfin_api_key": cfg.Jellyfin.APIKey,
		"emby_url":         cfg.Emby.URL,
		"emby_api_key":     cfg.Emby.APIKey,
		// Indexer
		"indexer_url":     indexerURL,
		"indexer_api_key": indexerAPIKey,
		"indexer_name":    indexerName,
		// Notifications
		"discord_webhook_url": cfg.Notifications.DiscordWebhookURL,
		"ntfy_topic_url":      cfg.Notifications.NtfyTopicURL,
		"gotify_url":          cfg.Notifications.GotifyURL,
		"gotify_token":        cfg.Notifications.GotifyToken,
		"apprise_url":         cfg.Notifications.AppriseURL,
		"telegram_bot_token":  cfg.Notifications.TelegramBotToken,
		"telegram_chat_id":    cfg.Notifications.TelegramChatID,
		"email_smtp_host":     cfg.Notifications.EmailSMTPHost,
		"email_smtp_port": func() string {
			if p := cfg.Notifications.EmailSMTPPort; p != 0 {
				return strconv.Itoa(p)
			}
			return ""
		}(),
		"email_username":      cfg.Notifications.EmailUsername,
		"email_password":      cfg.Notifications.EmailPassword,
		"email_from":          cfg.Notifications.EmailFrom,
		"email_to":            strings.Join(cfg.Notifications.EmailTo, ","),
	}
}

// ApplyStoredSettings loads persisted settings from the DB and applies them to
// the live config. Call this once at startup (after db.Connect) so that any
// settings saved via the UI survive container restarts without requiring env vars.
func ApplyStoredSettings(cfg *config.Config) {
	stored, err := db.NewConfigRepository().GetSettings()
	if err != nil || len(stored) == 0 {
		return
	}
	applyToCfg(stored, cfg)
}

// applyToCfg propagates known settings keys from a persisted map into the live
// config pointer so changes take effect immediately without a restart.
func applyToCfg(s map[string]interface{}, cfg *config.Config) {
	str := func(key string) (string, bool) {
		v, ok := s[key]
		if !ok {
			return "", false
		}
		sv, ok := v.(string)
		return sv, ok
	}

	// anyInt handles email_smtp_port which may arrive as float64 (JSON decode),
	// int (from envDefaults), or string (from older stored values).
	anyInt := func(key string) (int, bool) {
		v, ok := s[key]
		if !ok {
			return 0, false
		}
		switch n := v.(type) {
		case float64:
			return int(n), true
		case int:
			return n, true
		case string:
			if p, err := strconv.Atoi(n); err == nil {
				return p, true
			}
		}
		return 0, false
	}

	// Metadata API keys
	if v, ok := str("tmdb_api_key"); ok {
		cfg.TMDB.APIKey = v
	}
	if v, ok := str("tvdb_api_key"); ok {
		cfg.TVDB.APIKey = v
	}

	// Torrent client
	if v, ok := str("torrent_client_type"); ok {
		cfg.TorrentClient.ClientType = config.TorrentClientType(v)
	}
	if v, ok := str("torrent_client_url"); ok {
		cfg.TorrentClient.BaseURL = v
	}
	if v, ok := str("torrent_client_username"); ok {
		cfg.TorrentClient.Username = v
	}
	if v, ok := str("torrent_client_password"); ok {
		cfg.TorrentClient.Password = v
	}

	// Library paths
	if v, ok := str("movie_dir"); ok {
		cfg.FileMover.MovieDir = v
	}
	if v, ok := str("tvshow_dir"); ok {
		cfg.FileMover.TVShowDir = v
	}

	// Quality
	if v, ok := str("preferred_quality"); ok {
		cfg.Downloads.PreferredQuality = v
	}
	if v, ok := str("quality_fallback"); ok {
		var fallback []string
		for _, p := range strings.Split(v, ",") {
			if t := strings.TrimSpace(p); t != "" {
				fallback = append(fallback, t)
			}
		}
		if len(fallback) > 0 {
			cfg.Downloads.QualityFallback = fallback
		}
	}

	// Media servers
	if v, ok := str("plex_url"); ok {
		cfg.Plex.URL = v
	}
	if v, ok := str("plex_token"); ok {
		cfg.Plex.Token = v
	}
	if v, ok := str("jellyfin_url"); ok {
		cfg.Jellyfin.URL = v
	}
	if v, ok := str("jellyfin_api_key"); ok {
		cfg.Jellyfin.APIKey = v
	}
	if v, ok := str("emby_url"); ok {
		cfg.Emby.URL = v
	}
	if v, ok := str("emby_api_key"); ok {
		cfg.Emby.APIKey = v
	}

	// Indexer (single-indexer bootstrap)
	ensureIndexer := func() {
		if len(cfg.Indexers) == 0 {
			cfg.Indexers = append(cfg.Indexers, config.IndexerConfig{})
		}
	}
	if v, ok := str("indexer_url"); ok {
		ensureIndexer()
		cfg.Indexers[0].URL = v
	}
	if v, ok := str("indexer_api_key"); ok {
		ensureIndexer()
		cfg.Indexers[0].APIKey = v
	}
	if v, ok := str("indexer_name"); ok {
		ensureIndexer()
		cfg.Indexers[0].Name = v
	}

	// Notifications
	if v, ok := str("discord_webhook_url"); ok {
		cfg.Notifications.DiscordWebhookURL = v
	}
	if v, ok := str("ntfy_topic_url"); ok {
		cfg.Notifications.NtfyTopicURL = v
	}
	if v, ok := str("gotify_url"); ok {
		cfg.Notifications.GotifyURL = v
	}
	if v, ok := str("gotify_token"); ok {
		cfg.Notifications.GotifyToken = v
	}
	if v, ok := str("apprise_url"); ok {
		cfg.Notifications.AppriseURL = v
	}
	if v, ok := str("telegram_bot_token"); ok {
		cfg.Notifications.TelegramBotToken = v
	}
	if v, ok := str("telegram_chat_id"); ok {
		cfg.Notifications.TelegramChatID = v
	}
	if v, ok := str("email_smtp_host"); ok {
		cfg.Notifications.EmailSMTPHost = v
	}
	if v, ok := anyInt("email_smtp_port"); ok {
		cfg.Notifications.EmailSMTPPort = v
	}
	if v, ok := str("email_username"); ok {
		cfg.Notifications.EmailUsername = v
	}
	if v, ok := str("email_password"); ok {
		cfg.Notifications.EmailPassword = v
	}
	if v, ok := str("email_from"); ok {
		cfg.Notifications.EmailFrom = v
	}
	if v, ok := str("email_to"); ok {
		var emails []string
		for _, p := range strings.Split(v, ",") {
			if t := strings.TrimSpace(p); t != "" {
				emails = append(emails, t)
			}
		}
		cfg.Notifications.EmailTo = emails
	}
}

// syncIndexerToDB updates the first DB-persisted indexer from cfg.Indexers[0]
// so that changes made via the settings page take effect without a restart.
// If the DB has no indexers yet (not yet seeded), this is a no-op; the seed
// will happen on the first call to ListIndexers.
func syncIndexerToDB(cfg *config.Config) {
	if len(cfg.Indexers) == 0 {
		return
	}
	repo := db.NewIndexerRepository()
	existing, err := repo.GetAll()
	if err != nil || len(existing) == 0 {
		return
	}
	src := cfg.Indexers[0]
	first := existing[0]
	first.URL = src.URL
	first.APIKey = src.APIKey
	first.Name = src.Name
	_ = repo.Save(first)
}
