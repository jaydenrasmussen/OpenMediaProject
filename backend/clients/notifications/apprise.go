package notifications

import (
	"bytes"
	"encoding/json"
	"fmt"
	"net/http"
)

// AppriseNotifier sends notifications via an Apprise API server.
type AppriseNotifier struct {
	URL string
}

func (a *AppriseNotifier) Name() string {
	return "Apprise"
}

func (a *AppriseNotifier) Notify(event NotificationEvent) error {
	payload := map[string]string{
		"title": event.Title,
		"body":  event.Message,
	}

	body, err := json.Marshal(payload)
	if err != nil {
		return fmt.Errorf("apprise: failed to marshal payload: %w", err)
	}

	resp, err := http.Post(a.URL+"/notify", "application/json", bytes.NewReader(body))
	if err != nil {
		return fmt.Errorf("apprise: HTTP request failed: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return fmt.Errorf("apprise: unexpected status code %d", resp.StatusCode)
	}

	return nil
}
