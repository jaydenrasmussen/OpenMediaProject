package notifications

import (
	"bytes"
	"encoding/json"
	"fmt"
	"net/http"
)

type TelegramNotifier struct {
	BotToken string
	ChatID   string
}

func (t *TelegramNotifier) Name() string {
	return "telegram"
}

func (t *TelegramNotifier) Notify(event NotificationEvent) error {
	payload := map[string]string{
		"chat_id": t.ChatID,
		"text":    fmt.Sprintf("*%s*\n%s", event.Title, event.Message),
		"parse_mode": "Markdown",
	}

	body, err := json.Marshal(payload)
	if err != nil {
		return fmt.Errorf("telegram: failed to marshal payload: %w", err)
	}

	url := fmt.Sprintf("https://api.telegram.org/bot%s/sendMessage", t.BotToken)
	resp, err := http.Post(url, "application/json", bytes.NewReader(body))
	if err != nil {
		return fmt.Errorf("telegram: HTTP request failed: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return fmt.Errorf("telegram: unexpected status code %d", resp.StatusCode)
	}

	return nil
}
