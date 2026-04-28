package clients

import (
	"fmt"
	"net/http"

	"github.com/rs/zerolog/log"
)

// EmbyClient implements MediaServer for Emby.
type EmbyClient struct {
	baseURL string
	apiKey  string
}

// NewEmbyClient creates a new EmbyClient.
func NewEmbyClient(baseURL, apiKey string) *EmbyClient {
	return &EmbyClient{baseURL: baseURL, apiKey: apiKey}
}

// Name satisfies the MediaServer interface.
func (c *EmbyClient) Name() string { return "Emby" }

// ScanLibrary triggers a full library refresh on the Emby server.
func (c *EmbyClient) ScanLibrary() error {
	log.Info().Msg("Triggering Emby library scan")

	url := fmt.Sprintf("%s/Library/Refresh?api_key=%s", c.baseURL, c.apiKey)
	req, err := http.NewRequest(http.MethodPost, url, nil)
	if err != nil {
		return err
	}

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		log.Error().Err(err).Msg("Failed to trigger Emby library scan")
		return err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusNoContent && resp.StatusCode != http.StatusOK {
		return fmt.Errorf("emby scan returned unexpected status: %d", resp.StatusCode)
	}

	log.Info().Msg("Emby library scan triggered successfully")
	return nil
}
