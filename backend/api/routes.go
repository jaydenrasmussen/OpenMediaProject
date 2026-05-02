package api

import (
	"backend/middleware"

	"github.com/gofiber/fiber/v2"
	"github.com/rs/zerolog/log"
)

// Api holds the dependencies for the API handlers.
type Api struct {
	TorrentHandler  *TorrentHandler
	PlexHandler     *PlexHandler
	IndexerHandler  *IndexerHandler
	DownloadHandler *DownloadHandler
	WantedHandler   *WantedHandler
	EpisodeHandler  *EpisodeHandler
	LibraryHandler  *LibraryHandler
	HistoryHandler  *HistoryHandler
	SettingsHandler *SettingsHandler
	SystemHandler   *SystemHandler
}

// NewApi creates a new Api.
func NewApi(torrentHandler *TorrentHandler, plexHandler *PlexHandler, indexerHandler *IndexerHandler, downloadHandler *DownloadHandler, wantedHandler *WantedHandler, episodeHandler *EpisodeHandler, libraryHandler *LibraryHandler, historyHandler *HistoryHandler, settingsHandler *SettingsHandler, systemHandler *SystemHandler) *Api {
	return &Api{
		TorrentHandler:  torrentHandler,
		PlexHandler:     plexHandler,
		IndexerHandler:  indexerHandler,
		DownloadHandler: downloadHandler,
		WantedHandler:   wantedHandler,
		EpisodeHandler:  episodeHandler,
		LibraryHandler:  libraryHandler,
		HistoryHandler:  historyHandler,
		SettingsHandler: settingsHandler,
		SystemHandler:   systemHandler,
	}
}

// SetupRoutes configures all application routes
func (a *Api) SetupRoutes(app *fiber.App) {
	// API group
	// Setup API routes
	api := app.Group("/api")

	// Health check endpoint
	api.Get("/health", func(c *fiber.Ctx) error {
		return c.JSON(fiber.Map{
			"status":  "ok",
			"message": "Server is running",
		})
	})

	// v1 API group
	v1 := api.Group("/v1")

	// Auth routes (public — no JWT required)
	authRoutes := v1.Group("/auth")
	authRoutes.Get("/key", GetAPIKey)
	authRoutes.Post("/key", GenerateNewAPIKey)
	authRoutes.Get("/validate", ValidateAPIKey)
	authRoutes.Post("/login", Login)
	authRoutes.Post("/register", RegisterUser)

	// All remaining v1 routes require a valid JWT session.
	p := v1.Group("", middleware.RequireAuth)

	// Configuration routes
	configRoutes := p.Group("/config")
	configRoutes.Get("/", GetSystemConfig)
	configRoutes.Post("/", UpdateSystemConfig)
	configRoutes.Post("/test-torrent", TestTorrentConnection)

	// User routes
	userRoutes := p.Group("/users")
	userRoutes.Get("/", GetUsers)
	userRoutes.Get("/:id", GetUser)
	userRoutes.Post("/", CreateUser)
	userRoutes.Put("/:id", UpdateUser)
	userRoutes.Delete("/:id", DeleteUser)

	// TV Show routes
	tvShowRoutes := p.Group("/tvshows")
	tvShowRoutes.Get("/", GetTVShows)
	tvShowRoutes.Get("/recent", GetRecentTVShows)
	tvShowRoutes.Get("/search", SearchTVShows)
	tvShowRoutes.Get("/:id", GetTVShow)
	tvShowRoutes.Delete("/:id", DeleteTVShow)
	tvShowRoutes.Post("/fetch-popular", FetchPopularTVShows)
	tvShowRoutes.Post("/fetch-trending", FetchTrendingTV)
	tvShowRoutes.Patch("/:id/quality", SetTVShowQuality)
	tvShowRoutes.Post("/:id/download", a.DownloadHandler.DownloadTVShow)
	tvShowRoutes.Post("/:id/request", a.WantedHandler.RequestTVShow)
	tvShowRoutes.Get("/:id/episodes", a.EpisodeHandler.GetEpisodes)
	tvShowRoutes.Post("/:id/episodes/refresh", a.EpisodeHandler.RefreshEpisodes)
	tvShowRoutes.Put("/:id/episodes/:eid", a.EpisodeHandler.UpdateEpisode)
	tvShowRoutes.Get("/:id/seasons/:season/episodes", a.EpisodeHandler.GetSeasonEpisodes)
	tvShowRoutes.Post("/:id/monitor", a.EpisodeHandler.MonitorTVShow)

	// Movie routes
	movieRoutes := p.Group("/movies")
	movieRoutes.Get("/", GetMovies)
	movieRoutes.Get("/recent", GetRecentMovies)
	movieRoutes.Get("/search", SearchMovies)
	movieRoutes.Get("/:id", GetMovie)
	movieRoutes.Delete("/:id", DeleteMovie)
	movieRoutes.Post("/fetch-popular", FetchPopularMovies)
	movieRoutes.Post("/fetch-now-playing", FetchNowPlayingMovies)
	movieRoutes.Post("/fetch-trending", FetchTrendingMovies)
	movieRoutes.Post("/fetch-top-rated", FetchTopRatedMovies)
	movieRoutes.Post("/fetch-upcoming", FetchUpcomingMovies)
	movieRoutes.Patch("/:id/quality", SetMovieQuality)
	movieRoutes.Post("/:id/download", a.DownloadHandler.DownloadMovie)
	movieRoutes.Post("/:id/request", a.WantedHandler.RequestMovie)

	// Wanted routes
	wantedRoutes := p.Group("/wanted")
	wantedRoutes.Get("/", a.WantedHandler.ListWanted)
	wantedRoutes.Get("/:id", a.WantedHandler.GetWanted)
	wantedRoutes.Delete("/:id", a.WantedHandler.DeleteWanted)
	wantedRoutes.Post("/:id/approve", middleware.RequireRole("admin"), a.WantedHandler.ApproveWanted)
	wantedRoutes.Post("/:id/reject", middleware.RequireRole("admin"), a.WantedHandler.RejectWanted)

	// Plex routes
	plexRoutes := p.Group("/plex")
	plexRoutes.Get("/", a.PlexHandler.GetPlexConfig)
	plexRoutes.Post("/", a.PlexHandler.UpdatePlexConfig)

	// Indexer routes
	indexerRoutes := p.Group("/indexers")
	indexerRoutes.Get("/", a.IndexerHandler.ListIndexers)
	indexerRoutes.Get("/health", a.IndexerHandler.GetIndexerHealth)
	indexerRoutes.Post("/", a.IndexerHandler.CreateIndexer)
	indexerRoutes.Post("/search", a.IndexerHandler.SearchIndexers)
	indexerRoutes.Post("/grab", a.IndexerHandler.GrabTorrent)
	indexerRoutes.Post("/test/:id", a.IndexerHandler.TestIndexer)
	indexerRoutes.Put("/:id", a.IndexerHandler.UpdateIndexer)
	indexerRoutes.Delete("/:id", a.IndexerHandler.DeleteIndexer)

	// Release profile routes
	releaseProfileRoutes := p.Group("/release-profiles")
	releaseProfileRoutes.Get("/", ListReleaseProfiles)
	releaseProfileRoutes.Post("/", CreateReleaseProfile)
	releaseProfileRoutes.Put("/:id", UpdateReleaseProfile)
	releaseProfileRoutes.Delete("/:id", DeleteReleaseProfile)

	// Library routes
	libraryRoutes := p.Group("/library")
	libraryRoutes.Get("/scan", a.LibraryHandler.ScanLibrary)

	// History routes
	p.Get("/history", a.HistoryHandler.GetHistory)

	// Settings routes
	settingsRoutes := p.Group("/settings")
	settingsRoutes.Get("/", a.SettingsHandler.GetSettings)
	settingsRoutes.Put("/", a.SettingsHandler.UpdateSettings)

	// System routes
	systemRoutes := p.Group("/system")
	systemRoutes.Get("/tasks", a.SystemHandler.GetTasks)

	// Download routes
	downloadRoutes := p.Group("/downloads")
	downloadRoutes.Get("/", a.DownloadHandler.GetDownloads)
	downloadRoutes.Get("/:hash", a.DownloadHandler.GetDownload)

	// Torrent routes
	torrentRoutes := p.Group("/torrents")
	torrentRoutes.Get("/", a.TorrentHandler.GetTorrents)
	torrentRoutes.Get("/health", a.TorrentHandler.GetClientHealth)
	torrentRoutes.Get("/:hash", a.TorrentHandler.GetTorrentDetails)
	torrentRoutes.Post("/upload", a.TorrentHandler.UploadTorrent)
	torrentRoutes.Delete("/:hash", a.TorrentHandler.DeleteTorrent)
	torrentRoutes.Post("/:hash/:action", a.TorrentHandler.ControlTorrent)
}

// Error handler for the API
func ErrorHandler(c *fiber.Ctx, err error) error {
	// Default to 500 Internal Server Error
	code := fiber.StatusInternalServerError
	message := "Internal Server Error"

	// Check if it's a Fiber error
	if e, ok := err.(*fiber.Error); ok {
		code = e.Code
		message = e.Message
	}

	// Get request-scoped logger if available
	reqLogger := log.With().
		Str("path", c.Path()).
		Str("method", c.Method()).
		Str("client_ip", c.IP()).
		Int("status", code).
		Logger()

	// Log error with appropriate level based on status code
	if code >= 500 {
		reqLogger.Error().Err(err).Msg(message)
	} else if code >= 400 {
		reqLogger.Warn().Err(err).Msg(message)
	} else {
		reqLogger.Info().Err(err).Msg(message)
	}

	// Send JSON response
	return c.Status(code).JSON(fiber.Map{
		"error":   true,
		"message": message,
	})
}
