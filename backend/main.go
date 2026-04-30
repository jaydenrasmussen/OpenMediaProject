package main

import (
	"crypto/rand"
	"fmt"
	"os"
	"os/signal"
	"syscall"
	"time"

	"net/http"

	"backend/api"
	"backend/clients"
	"backend/clients/indexers"
	"backend/clients/notifications"
	"backend/config"
	"backend/db"
	"backend/logger"
	"backend/middleware"
	"backend/models"

	"github.com/gofiber/fiber/v2"
	"github.com/gofiber/fiber/v2/middleware/cors"
	"github.com/gofiber/fiber/v2/middleware/filesystem"
	"github.com/google/uuid"
	"github.com/rs/zerolog/log"
	"golang.org/x/crypto/bcrypt"
)

func main() {
	// Load configuration
	cfg, err := config.Load()
	if err != nil {
		log.Fatal().Err(err).Msg("Failed to load configuration")
	}

	// Initialize logger
	logger.Init(cfg)
	log.Info().Msg("Logger initialized")

	// Initialize database connection
	if err := db.Connect(cfg.Database); err != nil {
		log.Fatal().Err(err).Msg("Failed to connect to database")
	}
	log.Info().Str("path", cfg.Database.DBPath).Msg("Connected to database")

	// Migrate episodes from legacy flat bucket to per-show buckets (idempotent).
	if err := db.MigrateEpisodeBuckets(); err != nil {
		log.Fatal().Err(err).Msg("Failed to migrate episode buckets")
	}

	// Overlay any settings previously saved via the UI on top of the env-derived
	// config so they survive container restarts without requiring env vars.
	api.ApplyStoredSettings(cfg)

	// Bootstrap the first admin user if no admin exists yet.
	bootstrapAdmin()

	// Generate API key if needed
	if cfg.Auth.EnableAuth {
		apiKeyFromFile, err := os.ReadFile(".apikey")
		if err == nil {
			cfg.Auth.APIKey = string(apiKeyFromFile)
			log.Info().Msg("Loaded API key from .apikey file")
		} else {
			apiKey, err := middleware.GenerateCaseInsensitiveAPIKey(26)
			if err != nil {
				log.Fatal().Err(err).Msg("Failed to generate API key")
			}

			cfg.Auth.APIKey = apiKey
			if err := os.WriteFile(".apikey", []byte(apiKey), 0600); err != nil {
				log.Fatal().Err(err).Msg("Failed to write API key to .apikey file")
			}
			log.Info().Msg("Generated new API key and saved to .apikey file")
		}
	}

	// Create a new Fiber instance
	app := fiber.New(fiber.Config{
		AppName:      "OpenMediaProject API",
		ErrorHandler: api.ErrorHandler,
	})

	// Register middleware
	app.Use(middleware.ZeroLogger())
	app.Use(cors.New(cors.Config{
		AllowOrigins:     cfg.CorsAllowOrigins,
		AllowHeaders:     "Origin, Content-Type, Accept, Authorization, X-API-Key",
		AllowCredentials: true,
	}))

	// Apply API key authentication if enabled
	if cfg.Auth.EnableAuth {
		app.Use(middleware.NewAPIKeyAuthMiddleware(middleware.AuthConfig{
			APIKey:        cfg.Auth.APIKey,
			AllowQuery:    cfg.Auth.AllowQueryParam,
			SkipEndpoints: cfg.Auth.SkipAuthEndpoints,
		}))
		log.Info().Msg("API key authentication enabled")
	}

	// JWT session auth middleware (runs alongside API key auth; lenient — passes
	// unauthenticated requests through and lets handlers decide what's required).
	app.Use(middleware.JWTMiddleware(api.JWTSecret))

	// Initialize clients
	plexClient, err := clients.NewPlexClient(cfg.Plex.URL, cfg.Plex.Token)
	if err != nil {
		log.Warn().Err(err).Msg("Failed to create Plex client")
	}
	fileMover := clients.NewFileMover(cfg.FileMover.MovieDir, cfg.FileMover.TVShowDir)

	// Build media server registry
	mediaServers := clients.NewMediaServerRegistry()
	if plexClient != nil {
		mediaServers.Register(plexClient)
	}
	if cfg.Jellyfin.URL != "" {
		mediaServers.Register(clients.NewJellyfinClient(cfg.Jellyfin.URL, cfg.Jellyfin.APIKey))
	}
	if cfg.Emby.URL != "" {
		mediaServers.Register(clients.NewEmbyClient(cfg.Emby.URL, cfg.Emby.APIKey))
	}

	// Build notification registry
	notifRegistry := notifications.NewRegistry()
	api.RebuildNotifRegistry(cfg, notifRegistry)

	// Initialize handlers
	idxRegistry := indexers.NewDefaultRegistry()
	torrentHandler := api.NewTorrentHandler(mediaServers, fileMover, cfg, notifRegistry)
	plexRepository := db.NewPlexRepository(db.GetDB())
	plexHandler := api.NewPlexHandler(plexRepository)
	indexerHandler := api.NewIndexerHandler(cfg, idxRegistry)
	downloadHandler := api.NewDownloadHandler(cfg, fileMover, idxRegistry)
	wantedHandler := api.NewWantedHandler(cfg)
	episodeHandler := api.NewEpisodeHandler(cfg)
	libraryHandler := api.NewLibraryHandler(cfg, fileMover)
	wantedPoller := api.NewWantedPoller(cfg, fileMover, idxRegistry)
	tvdbRefresher := api.NewTVDBRefresher(cfg)
	historyHandler := api.NewHistoryHandler()
	settingsHandler := api.NewSettingsHandler(cfg, notifRegistry)
	taskTracker := api.NewTaskTracker()
	taskTracker.Register("torrent_completion_check", "every 5 min")
	taskTracker.Register("wanted_queue_poll", "every 15 min")
	taskTracker.Register("tvdb_episode_refresh", "every 24 h")
	taskTracker.Register("db_compaction", "every 7 days")
	systemHandler := api.NewSystemHandler(taskTracker)

	// Initialize API
	api := api.NewApi(torrentHandler, plexHandler, indexerHandler, downloadHandler, wantedHandler, episodeHandler, libraryHandler, historyHandler, settingsHandler, systemHandler)

	// Setup API routes
	api.SetupRoutes(app)

	// SvelteKit immutable assets have content-hashed filenames; cache them permanently.
	// index.html and other non-hashed files are left without a Cache-Control header
	// so browsers re-validate on each visit and pick up new deployments.
	app.Use("/_app/immutable", func(c *fiber.Ctx) error {
		c.Set(fiber.HeaderCacheControl, "public, max-age=31536000, immutable")
		return c.Next()
	})

	// Serve static frontend files
	app.Use("/", filesystem.New(filesystem.Config{
		Root:         http.Dir("./public"),
		Browse:       false,
		Index:        "index.html",
		NotFoundFile: "index.html", // For SPA routing
	}))

	// Start the server in a goroutine
	go func() {
		log.Info().Str("address", cfg.ServerAddress).Msg("Starting server")
		if err := app.Listen(cfg.ServerAddress); err != nil {
			log.Error().Err(err).Msg("Server error")
		}
	}()

	// Start background goroutines
	stopChecker := make(chan struct{})

	// Torrent completion poller — every 5 minutes, run immediately at startup
	go func() {
		ticker := time.NewTicker(5 * time.Minute)
		defer ticker.Stop()
		taskTracker.Start("torrent_completion_check")
		torrentHandler.CheckForCompletedTorrents() // run immediately
		taskTracker.Done("torrent_completion_check")
		for {
			select {
			case <-ticker.C:
				taskTracker.Start("torrent_completion_check")
				torrentHandler.CheckForCompletedTorrents()
				taskTracker.Done("torrent_completion_check")
			case <-stopChecker:
				return
			}
		}
	}()

	// Wanted queue poller — every 15 minutes, run immediately at startup
	go func() {
		ticker := time.NewTicker(15 * time.Minute)
		defer ticker.Stop()
		taskTracker.Start("wanted_queue_poll")
		wantedPoller.ProcessWantedQueue() // run immediately
		wantedPoller.ProcessUpgradeQueue()
		taskTracker.Done("wanted_queue_poll")
		for {
			select {
			case <-ticker.C:
				taskTracker.Start("wanted_queue_poll")
				wantedPoller.ProcessWantedQueue()
				wantedPoller.ProcessUpgradeQueue()
				taskTracker.Done("wanted_queue_poll")
			case <-stopChecker:
				return
			}
		}
	}()

	// TVDB episode refresh — every 24 hours
	go func() {
		ticker := time.NewTicker(24 * time.Hour)
		defer ticker.Stop()
		for {
			select {
			case <-ticker.C:
				taskTracker.Start("tvdb_episode_refresh")
				if err := tvdbRefresher.RefreshAll(); err != nil {
					log.Warn().Err(err).Msg("TVDB refresh failed")
					taskTracker.Fail("tvdb_episode_refresh", err.Error())
				} else {
					taskTracker.Done("tvdb_episode_refresh")
				}
			case <-stopChecker:
				return
			}
		}
	}()

	// DB compaction — weekly, to reclaim disk space from stale segment files
	go func() {
		ticker := time.NewTicker(7 * 24 * time.Hour)
		defer ticker.Stop()
		for {
			select {
			case <-ticker.C:
				taskTracker.Start("db_compaction")
				if err := db.GetDB().Merge(); err != nil {
					log.Warn().Err(err).Msg("DB compaction failed")
					taskTracker.Fail("db_compaction", err.Error())
				} else {
					log.Info().Msg("DB compaction complete")
					taskTracker.Done("db_compaction")
				}
			case <-stopChecker:
				return
			}
		}
	}()

	// Wait for interrupt signal to gracefully shut down the server
	quit := make(chan os.Signal, 1)
	signal.Notify(quit, os.Interrupt, syscall.SIGTERM)
	<-quit

	close(stopChecker)

	log.Info().Msg("Shutting down server...")
	if err := app.Shutdown(); err != nil {
		log.Fatal().Err(err).Msg("Server shutdown failed")
	}

	// Close database connection
	if err := db.Disconnect(); err != nil {
		log.Error().Err(err).Msg("Error disconnecting from database")
	}

	log.Info().Msg("Server exited properly")
}

// bootstrapAdmin creates the first admin user if no admin account exists.
// The password is read from the ADMIN_PASSWORD env var; if absent a random
// 16-character password is generated and printed once to the log.
func bootstrapAdmin() {
	repo := db.NewUserRepository()
	users, err := repo.GetAll()
	if err != nil {
		log.Error().Err(err).Msg("bootstrapAdmin: failed to list users")
		return
	}

	for _, u := range users {
		if u.Role == models.UserRoleAdmin {
			return // already have an admin, nothing to do
		}
	}

	password := os.Getenv("ADMIN_PASSWORD")
	if password == "" {
		b := make([]byte, 12)
		if _, err := rand.Read(b); err != nil {
			log.Error().Err(err).Msg("bootstrapAdmin: failed to generate password")
			return
		}
		password = fmt.Sprintf("%x", b) // 24-char hex string
	}

	hash, err := bcrypt.GenerateFromPassword([]byte(password), bcrypt.DefaultCost)
	if err != nil {
		log.Error().Err(err).Msg("bootstrapAdmin: failed to hash password")
		return
	}

	admin := &models.User{
		ID:           uuid.NewString(),
		Username:     "admin",
		Email:        "admin@localhost",
		PasswordHash: string(hash),
		Role:         models.UserRoleAdmin,
	}
	admin.BeforeCreate()
	admin.Role = models.UserRoleAdmin // override BeforeCreate default of "user"

	if err := repo.Create(admin); err != nil {
		log.Error().Err(err).Msg("bootstrapAdmin: failed to create admin user")
		return
	}

	if os.Getenv("ADMIN_PASSWORD") == "" {
		log.Warn().Str("username", "admin").Str("password", password).Msg("Created initial admin user — change this password immediately")
	} else {
		log.Info().Str("username", "admin").Msg("Created initial admin user")
	}
}
