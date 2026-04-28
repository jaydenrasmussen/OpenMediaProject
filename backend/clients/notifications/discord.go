package notifications

import (
	"bytes"
	"encoding/json"
	"fmt"
	"net/http"
)

type DiscordNotifier struct {
	WebhookURL string
}

func (d *DiscordNotifier) Name() string {
	return "discord"
}

func (d *DiscordNotifier) Notify(event NotificationEvent) error {
	payload := map[string]interface{}{
		"embeds": []map[string]interface{}{
			{
				"title":       event.Title,
				"description": event.Message,
				"color":       3447003,
			},
		},
	}

	body, err := json.Marshal(payload)
	if err != nil {
		return fmt.Errorf("discord: failed to marshal payload: %w", err)
	}

	resp, err := http.Post(d.WebhookURL, "application/json", bytes.NewReader(body))
	if err != nil {
		return fmt.Errorf("discord: HTTP request failed: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return fmt.Errorf("discord: unexpected status code %d", resp.StatusCode)
	}

	return nil
}
