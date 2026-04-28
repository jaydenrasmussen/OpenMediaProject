package api

import (
	"time"

	"backend/clients"
	"backend/config"
	"backend/db"
	"backend/models"

	"github.com/rs/zerolog/log"
)

// TVDBRefresher handles scheduled TVDB episode refresh for all known shows.
type TVDBRefresher struct {
	config *config.Config
}

// NewTVDBRefresher creates a new TVDBRefresher.
func NewTVDBRefresher(cfg *config.Config) *TVDBRefresher {
	return &TVDBRefresher{config: cfg}
}

// RefreshAll fetches new episode data for all TV shows in the DB.
// For each show, compares TVDB data against stored episodes.
// New episodes that have already aired (AirDate <= today) are created with status "missing".
// Episodes airing in the future get status "wanted".
// Existing episodes: only Title, AirDate, Overview, FetchedAt are updated.
func (r *TVDBRefresher) RefreshAll() error {
	showRepo := db.NewTVShowRepository()
	shows, err := showRepo.GetAll()
	if err != nil {
		return err
	}

	if r.config.TVDB.APIKey == "" {
		log.Warn().Msg("TVDB API key not configured; skipping scheduled refresh")
		return nil
	}

	tvdbClient := clients.NewTVDBClient(r.config.TVDB.APIKey)
	epRepo := db.NewEpisodeRepository()
	now := time.Now()
	today := now.Truncate(24 * time.Hour)

	for _, show := range shows {
		rawEpisodes, err := tvdbClient.GetEpisodes(show.ID)
		if err != nil {
			log.Error().Err(err).Str("show", show.Name).Msg("TVDB refresh: failed to fetch episodes, skipping show")
			time.Sleep(500 * time.Millisecond)
			continue
		}

		toSave := make([]*models.Episode, 0, len(rawEpisodes))

		for _, raw := range rawEpisodes {
			ep := models.NewEpisodeFromTVDBData(show.ID, raw)
			if ep == nil {
				continue
			}

			existing, err := epRepo.GetByKey(show.ID, ep.Season, ep.Number)
			if err == nil && existing != nil {
				// Update metadata only; preserve status, quality, DownloadedAt
				existing.Title = ep.Title
				existing.AirDate = ep.AirDate
				existing.Overview = ep.Overview
				existing.FetchedAt = now
				toSave = append(toSave, existing)
			} else {
				// New episode — determine status based on air date
				ep.Status = episodeStatusFromAirDate(ep.AirDate, today)
				ep.FetchedAt = now
				toSave = append(toSave, ep)
			}
		}

		if err := epRepo.SaveBatch(toSave); err != nil {
			log.Error().Err(err).Str("show", show.Name).Msg("TVDB refresh: failed to save episodes")
			time.Sleep(500 * time.Millisecond)
			continue
		}

		log.Info().Str("show", show.Name).Int("episodes", len(toSave)).Msg("TVDB refresh complete")

		time.Sleep(500 * time.Millisecond)
	}

	return nil
}
