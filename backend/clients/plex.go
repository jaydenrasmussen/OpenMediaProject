package clients

import (
	"fmt"
	"net/http"

	"github.com/jrudio/go-plex-client"
	"github.com/rs/zerolog/log"
)

// PlexClient represents the Plex client
type PlexClient struct {
	plex *plex.Plex
}

// NewPlexClient creates a new Plex client
func NewPlexClient(plexURL, plexToken string) (*PlexClient, error) {
	plex, err := plex.New(plexURL, plexToken)
	if err != nil {
		return nil, err
	}

	return &PlexClient{
		plex: plex,
	}, nil
}

// RefreshLibraries triggers a refresh of all libraries on the Plex server
func (c *PlexClient) RefreshLibraries() (bool, error) {
	query := fmt.Sprintf("%s/library/sections/all/refresh", c.plex.URL)

	// Create a new HTTP request
	req, err := http.NewRequest("GET", query, nil)
	if err != nil {
		return false, err
	}

	// Add the Plex token as a URL parameter
	q := req.URL.Query()
	q.Add("X-Plex-Token", c.plex.Token)
	req.URL.RawQuery = q.Encode()

	// Execute the request
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return false, err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return false, fmt.Errorf("unexpected status code: %d", resp.StatusCode)
	}

	return true, nil
}

// Name satisfies the MediaServer interface.
func (c *PlexClient) Name() string { return "Plex" }

// ScanLibrary triggers a library scan on the Plex server
func (c *PlexClient) ScanLibrary() error {
	log.Info().Msg("Triggering Plex library scan")
	_, err := c.RefreshLibraries()
	if err != nil {
		log.Error().Err(err).Msg("Failed to trigger Plex library scan")
		return err
	}

	log.Info().Msg("Plex library scan triggered successfully")
	return nil
}
