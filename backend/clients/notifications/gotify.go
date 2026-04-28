package notifications

import (
	"bytes"
	"encoding/json"
	"fmt"
	"net/http"
)

type GotifyNotifier struct {
	BaseURL string
	Token   string
}

func (g *GotifyNotifier) Name() string {
	return "Gotify"
}

func (g *GotifyNotifier) Notify(event NotificationEvent) error {
	payload := map[string]interface{}{
		"title":    event.Title,
		"message":  event.Message,
		"priority": 5,
	}

	body, err := json.Marshal(payload)
	if err != nil {
		return fmt.Errorf("gotify: failed to marshal payload: %w", err)
	}

	url := fmt.Sprintf("%s/message?token=%s", g.BaseURL, g.Token)
	resp, err := http.Post(url, "application/json", bytes.NewReader(body))
	if err != nil {
		return fmt.Errorf("gotify: HTTP request failed: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return fmt.Errorf("gotify: unexpected status code %d", resp.StatusCode)
	}

	return nil
}
