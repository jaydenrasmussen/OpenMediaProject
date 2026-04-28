package api

import (
	"backend/config"
	"backend/db"

	"github.com/gofiber/fiber/v2"
)

// SettingsHandler handles persistent application settings (stored in NutsDB).
type SettingsHandler struct {
	cfg *config.Config
}

// NewSettingsHandler creates a new SettingsHandler.
func NewSettingsHandler(cfg *config.Config) *SettingsHandler {
	return &SettingsHandler{cfg: cfg}
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

	return c.JSON(fiber.Map{
		"success": true,
		"message": "Settings saved",
	})
}

// envDefaults builds a settings map from the currently-loaded env config so the
// UI always has sensible starting values even before anything is persisted.
func (h *SettingsHandler) envDefaults() map[string]interface{} {
	cfg := h.cfg
	return map[string]interface{}{
		// Torrent client
		"torrent_client_type":     string(cfg.TorrentClient.ClientType),
		"torrent_client_url":      cfg.TorrentClient.BaseURL,
		"torrent_client_username": cfg.TorrentClient.Username,
		// Library paths
		"movie_dir":   cfg.FileMover.MovieDir,
		"tvshow_dir":  cfg.FileMover.TVShowDir,
		// Quality
		"preferred_quality": cfg.Downloads.PreferredQuality,
		"quality_fallback":  cfg.Downloads.QualityFallback,
		// Media servers
		"plex_url":         cfg.Plex.URL,
		"jellyfin_url":     cfg.Jellyfin.URL,
		"emby_url":         cfg.Emby.URL,
		// Notifications
		"discord_webhook_url": cfg.Notifications.DiscordWebhookURL,
		"ntfy_topic_url":      cfg.Notifications.NtfyTopicURL,
		"gotify_url":          cfg.Notifications.GotifyURL,
		"apprise_url":         cfg.Notifications.AppriseURL,
		"email_smtp_host":     cfg.Notifications.EmailSMTPHost,
		"email_from":          cfg.Notifications.EmailFrom,
	}
}

// applyToCfg propagates known settings keys from a persisted map into the live
// config pointer so changes take effect immediately without a restart.
// Sensitive credentials (passwords, tokens) are intentionally excluded — the
// UI does not expose them and they must be set via environment variables.
func applyToCfg(s map[string]interface{}, cfg *config.Config) {
	str := func(key string) (string, bool) {
		v, ok := s[key]
		if !ok {
			return "", false
		}
		sv, ok := v.(string)
		return sv, ok
	}

	if v, ok := str("torrent_client_type"); ok {
		cfg.TorrentClient.ClientType = config.TorrentClientType(v)
	}
	if v, ok := str("torrent_client_url"); ok {
		cfg.TorrentClient.BaseURL = v
	}
	if v, ok := str("torrent_client_username"); ok {
		cfg.TorrentClient.Username = v
	}
	if v, ok := str("movie_dir"); ok {
		cfg.FileMover.MovieDir = v
	}
	if v, ok := str("tvshow_dir"); ok {
		cfg.FileMover.TVShowDir = v
	}
	if v, ok := str("preferred_quality"); ok {
		cfg.Downloads.PreferredQuality = v
	}
	// quality_fallback is stored as a JSON array by the UI; skip live-apply
	// to avoid complex type coercion — restart picks it up from env.
	if v, ok := str("plex_url"); ok {
		cfg.Plex.URL = v
	}
	if v, ok := str("jellyfin_url"); ok {
		cfg.Jellyfin.URL = v
	}
	if v, ok := str("emby_url"); ok {
		cfg.Emby.URL = v
	}
	if v, ok := str("discord_webhook_url"); ok {
		cfg.Notifications.DiscordWebhookURL = v
	}
	if v, ok := str("ntfy_topic_url"); ok {
		cfg.Notifications.NtfyTopicURL = v
	}
	if v, ok := str("gotify_url"); ok {
		cfg.Notifications.GotifyURL = v
	}
	if v, ok := str("apprise_url"); ok {
		cfg.Notifications.AppriseURL = v
	}
	if v, ok := str("email_smtp_host"); ok {
		cfg.Notifications.EmailSMTPHost = v
	}
	if v, ok := str("email_from"); ok {
		cfg.Notifications.EmailFrom = v
	}
}
