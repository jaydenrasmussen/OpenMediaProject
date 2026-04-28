package clients

import "github.com/rs/zerolog/log"

// MediaServerRegistry holds all configured media servers and fans out scan requests.
type MediaServerRegistry struct {
	servers []MediaServer
}

// NewMediaServerRegistry creates an empty registry.
func NewMediaServerRegistry() *MediaServerRegistry {
	return &MediaServerRegistry{}
}

// Register adds a media server to the registry.
func (r *MediaServerRegistry) Register(s MediaServer) {
	r.servers = append(r.servers, s)
}

// Scan triggers ScanLibrary on every registered media server, logging but not
// propagating individual errors so one failing server does not block the others.
func (r *MediaServerRegistry) Scan() {
	for _, s := range r.servers {
		if err := s.ScanLibrary(); err != nil {
			log.Error().Err(err).Str("server", s.Name()).Msg("MediaServer scan failed")
		}
	}
}
