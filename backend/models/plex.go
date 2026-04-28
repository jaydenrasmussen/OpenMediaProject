package models

// PlexConfig represents the Plex client configuration.
type PlexConfig struct {
	URL   string `json:"url"`
	Token string `json:"token"`
}
