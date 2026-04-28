package notifications

import (
	"fmt"
	"net/http"
	"strings"
)

type NtfyNotifier struct {
	TopicURL string
}

func (n *NtfyNotifier) Name() string {
	return "ntfy"
}

func (n *NtfyNotifier) Notify(event NotificationEvent) error {
	req, err := http.NewRequest(http.MethodPost, n.TopicURL, strings.NewReader(event.Message))
	if err != nil {
		return fmt.Errorf("ntfy: failed to create request: %w", err)
	}
	req.Header.Set("Title", event.Title)
	req.Header.Set("Content-Type", "text/plain")

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return fmt.Errorf("ntfy: request failed: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return fmt.Errorf("ntfy: unexpected status code %d", resp.StatusCode)
	}

	return nil
}
