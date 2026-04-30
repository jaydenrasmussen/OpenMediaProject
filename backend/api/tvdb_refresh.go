package api

import (
	"time"

	"backend/clients"
	"backend/config"
	"backend/db"
	"backend/models"

	"github.com/google/uuid"
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
		var newForMonitored []*models.Episode

		// Load all known episodes for this show in one read to avoid N+1 DB round-trips.
		type existingEpKey struct{ season, episode int }
		existingMap := make(map[existingEpKey]*models.Episode)
		if stored, err := epRepo.GetByShow(show.ID); err == nil {
			for _, e := range stored {
				existingMap[existingEpKey{e.Season, e.Number}] = e
			}
		}

		for _, raw := range rawEpisodes {
			ep := models.NewEpisodeFromTVDBData(show.ID, raw)
			if ep == nil {
				continue
			}

			if existing := existingMap[existingEpKey{ep.Season, ep.Number}]; existing != nil {
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
				if show.Monitored {
					newForMonitored = append(newForMonitored, ep)
				}
			}
		}

		if err := epRepo.SaveBatch(toSave); err != nil {
			log.Error().Err(err).Str("show", show.Name).Msg("TVDB refresh: failed to save episodes")
			time.Sleep(500 * time.Millisecond)
			continue
		}

		// For monitored shows, add newly discovered episodes to the wanted queue.
		if len(newForMonitored) > 0 {
			wantedRepo := db.NewWantedRepository()

			// Build a deduplication set of (season, episode) already active in the queue.
			type epKey struct{ season, episode int }
			activeSet := make(map[epKey]bool)
			if existing, err := wantedRepo.GetByMedia("tv", show.ID); err == nil {
				for _, w := range existing {
					if w.Status == models.WantedStatusWanted || w.Status == models.WantedStatusDownloading {
						activeSet[epKey{w.Season, w.Episode}] = true
					}
				}
			}

			for _, ep := range newForMonitored {
				if ep.Status == models.EpisodeStatusDownloaded ||
					ep.Status == models.EpisodeStatusSkipped ||
					ep.Status == models.EpisodeStatusDownloading {
					continue
				}
				if activeSet[epKey{ep.Season, ep.Number}] {
					continue
				}
				item := &models.WantedItem{
					ID:         uuid.NewString(),
					MediaType:  "tv",
					MediaID:    show.ID,
					Title:      show.Name,
					Season:     ep.Season,
					Episode:    ep.Number,
					MinQuality: r.config.Downloads.PreferredQuality,
					Status:     models.WantedStatusWanted,
					AddedAt:    now,
					UpdatedAt:  now,
				}
				if err := wantedRepo.Save(item); err != nil {
					log.Warn().Err(err).Str("show", show.Name).Int("season", ep.Season).Int("episode", ep.Number).Msg("TVDB refresh: failed to queue new episode for monitored show")
				} else {
					log.Info().Str("show", show.Name).Int("season", ep.Season).Int("episode", ep.Number).Msg("TVDB refresh: queued new episode for monitored show")
				}
			}
		}

		log.Info().Str("show", show.Name).Int("episodes", len(toSave)).Msg("TVDB refresh complete")

		time.Sleep(500 * time.Millisecond)
	}

	return nil
}
