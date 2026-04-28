package clients

import (
	"fmt"
	"net/http"

	"github.com/rs/zerolog/log"
)

// JellyfinClient implements MediaServer for Jellyfin.
type JellyfinClient struct {
	baseURL string
	apiKey  string
}

// NewJellyfinClient creates a new JellyfinClient.
func NewJellyfinClient(baseURL, apiKey string) *JellyfinClient {
	return &JellyfinClient{baseURL: baseURL, apiKey: apiKey}
}

// Name satisfies the MediaServer interface.
func (c *JellyfinClient) Name() string { return "Jellyfin" }

// ScanLibrary triggers a full library refresh on the Jellyfin server.
func (c *JellyfinClient) ScanLibrary() error {
	log.Info().Msg("Triggering Jellyfin library scan")

	url := fmt.Sprintf("%s/Library/Refresh?api_key=%s", c.baseURL, c.apiKey)
	req, err := http.NewRequest(http.MethodPost, url, nil)
	if err != nil {
		return err
	}

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		log.Error().Err(err).Msg("Failed to trigger Jellyfin library scan")
		return err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusNoContent && resp.StatusCode != http.StatusOK {
		return fmt.Errorf("jellyfin scan returned unexpected status: %d", resp.StatusCode)
	}

	log.Info().Msg("Jellyfin library scan triggered successfully")
	return nil
}
