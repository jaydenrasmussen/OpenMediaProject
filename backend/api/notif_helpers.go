package api

import (
	"backend/clients/notifications"
	"backend/config"
)

// RebuildNotifRegistry clears r and re-populates it from cfg.
// Called at startup and again whenever notification-related settings are saved
// via the UI so changes take effect without a restart.
func RebuildNotifRegistry(cfg *config.Config, r *notifications.Registry) {
	r.Reset()

	if cfg.Notifications.DiscordWebhookURL != "" {
		r.Register(&notifications.DiscordNotifier{WebhookURL: cfg.Notifications.DiscordWebhookURL})
	}
	if cfg.Notifications.NtfyTopicURL != "" {
		r.Register(&notifications.NtfyNotifier{TopicURL: cfg.Notifications.NtfyTopicURL})
	}
	if cfg.Notifications.GotifyURL != "" && cfg.Notifications.GotifyToken != "" {
		r.Register(&notifications.GotifyNotifier{BaseURL: cfg.Notifications.GotifyURL, Token: cfg.Notifications.GotifyToken})
	}
	if cfg.Notifications.AppriseURL != "" {
		r.Register(&notifications.AppriseNotifier{URL: cfg.Notifications.AppriseURL})
	}
	if cfg.Notifications.EmailSMTPHost != "" {
		r.Register(&notifications.EmailNotifier{
			SMTPHost: cfg.Notifications.EmailSMTPHost,
			SMTPPort: cfg.Notifications.EmailSMTPPort,
			Username: cfg.Notifications.EmailUsername,
			Password: cfg.Notifications.EmailPassword,
			From:     cfg.Notifications.EmailFrom,
			To:       cfg.Notifications.EmailTo,
		})
	}
	if cfg.Notifications.TelegramBotToken != "" && cfg.Notifications.TelegramChatID != "" {
		r.Register(&notifications.TelegramNotifier{BotToken: cfg.Notifications.TelegramBotToken, ChatID: cfg.Notifications.TelegramChatID})
	}
}
