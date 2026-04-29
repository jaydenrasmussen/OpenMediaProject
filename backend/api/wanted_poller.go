package api

import (
	"fmt"
	"time"

	"backend/clients"
	"backend/clients/indexers"
	"backend/config"
	"backend/db"
	"backend/models"

	"github.com/rs/zerolog/log"
)

// WantedPoller processes the wanted queue and quality upgrade queue.
type WantedPoller struct {
	config    *config.Config
	registry  *indexers.Registry
	fileMover *clients.FileMover
}

// NewWantedPoller creates a new WantedPoller.
func NewWantedPoller(cfg *config.Config, fileMover *clients.FileMover, registry *indexers.Registry) *WantedPoller {
	return &WantedPoller{
		config:    cfg,
		registry:  registry,
		fileMover: fileMover,
	}
}

// ProcessWantedQueue searches indexers for every item currently in the
// "wanted" state and, if a qualifying result is found, enqueues it for
// download via the configured torrent client.
func (p *WantedPoller) ProcessWantedQueue() {
	wantedRepo := db.NewWantedRepository()

	items, err := wantedRepo.GetByStatus(models.WantedStatusWanted)
	if err != nil {
		log.Error().Err(err).Msg("WantedPoller: failed to load wanted items")
		return
	}

	if len(items) > 0 {
		_ = db.NewHistoryRepository().Add(&models.HistoryEntry{
			EventType: models.HistoryEventSearchStarted,
			Message:   fmt.Sprintf("Searching wanted queue: %d item(s)", len(items)),
		})
	}

	for _, item := range items {
		p.processWantedItem(item, wantedRepo)
	}
}

// processWantedItem handles a single wanted item: searches, filters by
// minimum quality, grabs the best result, and updates the item's status.
func (p *WantedPoller) processWantedItem(item *models.WantedItem, wantedRepo *db.WantedRepository) {
	minQuality := indexers.ParseQuality(item.MinQuality)

	req := &indexers.SearchRequest{
		Quality: minQuality,
	}

	switch item.MediaType {
	case "movie":
		req.MediaType = indexers.MediaTypeMovie
		req.Query = item.Title
	case "tv":
		req.MediaType = indexers.MediaTypeTV
		req.Query = item.Title
		req.Season = item.Season
		req.Episode = item.Episode
	default:
		log.Warn().Str("id", item.ID).Str("media_type", item.MediaType).Msg("WantedPoller: unknown media type, skipping")
		return
	}

	results, errs := p.registry.Search(req)
	for _, e := range errs {
		log.Warn().Err(e).Str("title", item.Title).Msg("WantedPoller: indexer error during search")
	}
	results = applyReleaseProfiles(results)

	if len(results) == 0 {
		log.Debug().Str("title", item.Title).Msg("WantedPoller: no results found, will retry next tick")
		return
	}

	// Filter: keep only results that meet or exceed the minimum quality.
	// rankQuality returns a rank where lower = better quality.
	// 2160p=-1, 1080p=0, 720p=1, 480p=2, any/unknown=3
	rankQuality := func(q indexers.Quality) int {
		if q == indexers.Quality2160p {
			return -1
		}
		for i, fq := range indexers.QualityFallbackOrder {
			if fq == q {
				return i
			}
		}
		return len(indexers.QualityFallbackOrder)
	}

	qualifying := make([]*indexers.SearchResult, 0, len(results))
	for _, r := range results {
		// Accept result if its quality rank is <= minQuality rank (i.e. as good or better)
		if rankQuality(r.Quality) <= rankQuality(minQuality) {
			qualifying = append(qualifying, r)
		}
	}

	if len(qualifying) == 0 {
		log.Debug().Str("title", item.Title).Str("min_quality", item.MinQuality).Msg("WantedPoller: no results meet minimum quality, will retry next tick")
		return
	}

	best := qualifying[0]

	if item.MediaType == "movie" {
		movie, dbErr := db.NewMovieRepository().GetByID(item.MediaID)
		if dbErr == nil && p.fileMover.IsMovieDuplicate(movie, string(best.Quality)) {
			log.Info().Str("title", item.Title).Msg("WantedPoller: file already exists in library at equal or better quality, marking available")
			_ = wantedRepo.UpdateStatus(item.ID, models.WantedStatusAvailable)
			return
		}
	}

	if item.MediaType == "tv" && item.Season > 0 && item.Episode > 0 {
		show, dbErr := db.NewTVShowRepository().GetByID(item.MediaID)
		ep, epErr := db.NewEpisodeRepository().GetByKey(item.MediaID, item.Season, item.Episode)
		if dbErr == nil && epErr == nil && p.fileMover.IsTVDuplicate(show, ep, string(best.Quality)) {
			log.Info().Str("title", item.Title).Msg("WantedPoller: episode already exists in library, marking available")
			_ = wantedRepo.UpdateStatus(item.ID, models.WantedStatusAvailable)
			return
		}
	}

	torrentClient, err := buildTorrentClient(p.config)
	if err != nil {
		log.Error().Err(err).Str("title", item.Title).Msg("WantedPoller: failed to build torrent client")
		return
	}

	if err := grabTorrent(torrentClient, best); err != nil {
		log.Error().Err(err).Str("title", item.Title).Msg("WantedPoller: failed to grab torrent")
		return
	}

	// Mark episode as downloading.
	if item.MediaType == "tv" && item.Season > 0 && item.Episode > 0 {
		if ep, epErr := db.NewEpisodeRepository().GetByKey(item.MediaID, item.Season, item.Episode); epErr == nil {
			ep.Status = models.EpisodeStatusDownloading
			if saveErr := db.NewEpisodeRepository().Save(ep); saveErr != nil {
				log.Warn().Err(saveErr).Str("mediaID", item.MediaID).Int("season", item.Season).Int("episode", item.Episode).Msg("processWantedItem: failed to update episode status to downloading")
			}
		}
	}

	now := time.Now()

	if best.InfoHash != "" {
		record := &models.PendingDownload{
			Hash:         best.InfoHash,
			MediaType:    item.MediaType,
			MediaID:      item.MediaID,
			Title:        item.Title,
			Quality:      string(best.Quality),
			Source:       best.Source,
			Status:       models.DownloadStatusDownloading,
			UpgradeUntil: item.UpgradeUntil,
			AddedAt:      now,
			UpdatedAt:    now,
		}
		if err := db.NewDownloadRepository().Save(record); err != nil {
			log.Warn().Err(err).Str("hash", best.InfoHash).Msg("WantedPoller: failed to save pending download record")
		}
	} else {
		log.Warn().Str("title", item.Title).Msg("WantedPoller: torrent has no InfoHash; download will not be tracked")
	}

	if err := wantedRepo.UpdateStatus(item.ID, models.WantedStatusDownloading); err != nil {
		log.Error().Err(err).Str("id", item.ID).Msg("WantedPoller: failed to update wanted item status")
		return
	}

	log.Info().
		Str("title", item.Title).
		Str("source", best.Source).
		Str("quality", string(best.Quality)).
		Msg("Wanted item download started")

	_ = db.NewHistoryRepository().Add(&models.HistoryEntry{
		EventType: models.HistoryEventTorrentAdded,
		Title:     item.Title,
		MediaType: item.MediaType,
		MediaID:   item.MediaID,
		Message:   fmt.Sprintf("Torrent added for %s (%s) from %s", item.Title, best.Quality, best.Source),
		User:      item.AddedBy,
	})
}

// ProcessUpgradeQueue checks completed downloads that have an UpgradeUntil
// target and attempts to find a higher-quality version if the target has not
// yet been reached.
func (p *WantedPoller) ProcessUpgradeQueue() {
	downloadRepo := db.NewDownloadRepository()

	all, err := downloadRepo.GetAll()
	if err != nil {
		log.Error().Err(err).Msg("WantedPoller: failed to load downloads for upgrade check")
		return
	}

	for _, d := range all {
		if d.Status != models.DownloadStatusCompleted {
			continue
		}
		if d.UpgradeUntil == "" {
			continue
		}
		p.processUpgrade(d, downloadRepo)
	}
}

// processUpgrade attempts a quality upgrade for a single completed download.
func (p *WantedPoller) processUpgrade(d *models.PendingDownload, downloadRepo *db.DownloadRepository) {
	targetQuality := indexers.ParseQuality(d.UpgradeUntil)
	currentQuality := indexers.ParseQuality(d.Quality)

	// Already at or above the target quality — nothing to do.
	if indexers.QualityScore(targetQuality, currentQuality) == 100 {
		log.Debug().
			Str("title", d.Title).
			Str("quality", d.Quality).
			Str("upgrade_until", d.UpgradeUntil).
			Msg("WantedPoller: already at target quality, skipping upgrade")
		return
	}

	var mediaType indexers.MediaType
	switch d.MediaType {
	case "movie":
		mediaType = indexers.MediaTypeMovie
	case "tv":
		mediaType = indexers.MediaTypeTV
	default:
		log.Warn().Str("media_type", d.MediaType).Str("title", d.Title).Msg("WantedPoller: unknown media type for upgrade, skipping")
		return
	}

	req := &indexers.SearchRequest{
		Query:     d.Title,
		MediaType: mediaType,
		Quality:   targetQuality,
	}

	results, errs := p.registry.Search(req)
	for _, e := range errs {
		log.Warn().Err(e).Str("title", d.Title).Msg("WantedPoller: indexer error during upgrade search")
	}
	results = applyReleaseProfiles(results)

	if len(results) == 0 {
		log.Debug().Str("title", d.Title).Msg("WantedPoller: no upgrade candidates found")
		return
	}

	// Find the first result with higher quality than what we currently have.
	var best *indexers.SearchResult
	for _, r := range results {
		if indexers.QualityScore(targetQuality, r.Quality) > indexers.QualityScore(targetQuality, currentQuality) {
			best = r
			break
		}
	}

	if best == nil {
		log.Debug().
			Str("title", d.Title).
			Str("current_quality", d.Quality).
			Msg("WantedPoller: no result with higher quality than current, skipping")
		return
	}

	log.Info().
		Str("title", d.Title).
		Str("current_quality", d.Quality).
		Str("upgrade_quality", string(best.Quality)).
		Str("target_quality", d.UpgradeUntil).
		Str("source", best.Source).
		Msg("WantedPoller: attempting quality upgrade")

	torrentClient, err := buildTorrentClient(p.config)
	if err != nil {
		log.Error().Err(err).Str("title", d.Title).Msg("WantedPoller: failed to build torrent client for upgrade")
		return
	}

	if err := grabTorrent(torrentClient, best); err != nil {
		log.Error().Err(err).Str("title", d.Title).Msg("WantedPoller: failed to grab upgrade torrent")
		return
	}

	now := time.Now()

	if best.InfoHash != "" {
		record := &models.PendingDownload{
			Hash:         best.InfoHash,
			MediaType:    d.MediaType,
			MediaID:      d.MediaID,
			Title:        d.Title,
			Quality:      string(best.Quality),
			Source:       best.Source,
			Status:       models.DownloadStatusDownloading,
			UpgradeUntil: d.UpgradeUntil,
			AddedAt:      now,
			UpdatedAt:    now,
		}
		if err := downloadRepo.Save(record); err != nil {
			log.Warn().Err(err).Str("hash", best.InfoHash).Msg("WantedPoller: failed to save upgrade download record")
		}
	} else {
		log.Warn().Str("title", d.Title).Msg("WantedPoller: upgrade torrent has no InfoHash; download will not be tracked")
	}

	// Clear UpgradeUntil on the original record so it is not re-processed.
	d.UpgradeUntil = ""
	d.UpdatedAt = time.Now()
	if err := downloadRepo.Save(d); err != nil {
		log.Warn().Err(err).Str("hash", d.Hash).Msg("WantedPoller: failed to clear UpgradeUntil on original record")
	}

	_ = db.NewHistoryRepository().Add(&models.HistoryEntry{
		EventType: models.HistoryEventUpgradeFound,
		Title:     d.Title,
		MediaType: d.MediaType,
		MediaID:   d.MediaID,
		Message:   fmt.Sprintf("Quality upgrade found for %s: %s → %s", d.Title, d.Quality, string(best.Quality)),
	})
}
