package api

import (
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"

	"backend/clients"
	"backend/clients/indexers"
	"backend/config"
	"backend/db"
	"backend/models"

	"github.com/gofiber/fiber/v2"
)

// IndexerHandler handles indexer-related HTTP requests
type IndexerHandler struct {
	cfg      *config.Config
	registry *indexers.Registry
}

// NewIndexerHandler creates a new IndexerHandler
func NewIndexerHandler(cfg *config.Config, registry *indexers.Registry) *IndexerHandler {
	return &IndexerHandler{cfg: cfg, registry: registry}
}

// ListIndexers returns all configured indexers
func (h *IndexerHandler) ListIndexers(c *fiber.Ctx) error {
	repo := db.NewIndexerRepository()

	indexers, err := repo.GetAll()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to retrieve indexers: " + err.Error(),
		})
	}

	// Seed from env config if DB is empty and env indexer is configured
	if len(indexers) == 0 && len(h.cfg.Indexers) > 0 {
		envIdx := h.cfg.Indexers[0]
		seeded := &models.Indexer{
			Name:     envIdx.Name,
			URL:      envIdx.URL,
			APIKey:   envIdx.APIKey,
			Type:     models.IndexerTypeTorznab,
			Enabled:  true,
			Priority: 0,
			AddedAt:  time.Now(),
		}
		if err := repo.Save(seeded); err == nil {
			indexers = append(indexers, seeded)
		}
	}

	return c.JSON(fiber.Map{
		"indexers": indexers,
		"count":    len(indexers),
	})
}

// createIndexerBody is used to parse the request body for CreateIndexer
type createIndexerBody struct {
	Name     string             `json:"name"`
	URL      string             `json:"url"`
	APIKey   string             `json:"api_key"`
	Type     models.IndexerType `json:"type"`
	Priority int                `json:"priority"`
	Enabled  bool               `json:"enabled"`
}

// CreateIndexer adds a new indexer
func (h *IndexerHandler) CreateIndexer(c *fiber.Ctx) error {
	var body createIndexerBody
	if err := c.BodyParser(&body); err != nil {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Invalid request body: " + err.Error(),
		})
	}

	if body.Name == "" || body.URL == "" {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "name and url are required",
		})
	}

	if body.Type == "" {
		body.Type = models.IndexerTypeTorznab
	}

	indexer := &models.Indexer{
		Name:     body.Name,
		URL:      body.URL,
		APIKey:   body.APIKey,
		Type:     body.Type,
		Priority: body.Priority,
		Enabled:  body.Enabled,
		AddedAt:  time.Now(),
	}

	repo := db.NewIndexerRepository()
	if err := repo.Save(indexer); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to save indexer: " + err.Error(),
		})
	}

	return c.Status(fiber.StatusCreated).JSON(indexer)
}

// UpdateIndexer updates an existing indexer by ID
func (h *IndexerHandler) UpdateIndexer(c *fiber.Ctx) error {
	id := c.Params("id")

	repo := db.NewIndexerRepository()
	existing, err := repo.GetByID(id)
	if err != nil {
		if err == db.ErrIndexerNotFound {
			return c.Status(fiber.StatusNotFound).JSON(fiber.Map{
				"error":   true,
				"message": "Indexer not found",
			})
		}
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to retrieve indexer: " + err.Error(),
		})
	}

	var body createIndexerBody
	if err := c.BodyParser(&body); err != nil {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Invalid request body: " + err.Error(),
		})
	}

	if body.Name != "" {
		existing.Name = body.Name
	}
	if body.URL != "" {
		existing.URL = body.URL
	}
	if body.APIKey != "" {
		existing.APIKey = body.APIKey
	}
	if body.Type != "" {
		existing.Type = body.Type
	}
	existing.Priority = body.Priority
	existing.Enabled = body.Enabled

	if err := repo.Save(existing); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to update indexer: " + err.Error(),
		})
	}

	return c.JSON(existing)
}

// DeleteIndexer removes an indexer by ID
func (h *IndexerHandler) DeleteIndexer(c *fiber.Ctx) error {
	id := c.Params("id")

	repo := db.NewIndexerRepository()
	if _, err := repo.GetByID(id); err != nil {
		if err == db.ErrIndexerNotFound {
			return c.Status(fiber.StatusNotFound).JSON(fiber.Map{
				"error":   true,
				"message": "Indexer not found",
			})
		}
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to retrieve indexer: " + err.Error(),
		})
	}

	if err := repo.Delete(id); err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to delete indexer: " + err.Error(),
		})
	}

	return c.SendStatus(fiber.StatusNoContent)
}

// SearchIndexers fans out a search to all enabled indexers (or a specific one)
func (h *IndexerHandler) SearchIndexers(c *fiber.Ctx) error {
	var req models.IndexerSearchRequest
	if err := c.BodyParser(&req); err != nil {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Invalid request body: " + err.Error(),
		})
	}

	repo := db.NewIndexerRepository()
	all, err := repo.GetAll()
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to retrieve indexers: " + err.Error(),
		})
	}

	// Filter to enabled indexers (and optionally to a specific one)
	var targets []*models.Indexer
	for _, idx := range all {
		if !idx.Enabled {
			continue
		}
		if req.IndexerID != "" && idx.ID != req.IndexerID {
			continue
		}
		targets = append(targets, idx)
	}

	if len(targets) == 0 {
		return c.JSON(fiber.Map{
			"results": []*models.IndexerSearchResult{},
			"count":   0,
			"errors":  []string{},
		})
	}

	results, errs := clients.MultiIndexerSearch(targets, &req)

	errStrings := make([]string, 0, len(errs))
	for _, e := range errs {
		errStrings = append(errStrings, e.Error())
	}

	return c.JSON(fiber.Map{
		"results": results,
		"count":   len(results),
		"errors":  errStrings,
	})
}

// GrabTorrent downloads a torrent from a URL or magnet and sends it to the torrent client
func (h *IndexerHandler) GrabTorrent(c *fiber.Ctx) error {
	var req models.GrabRequest
	if err := c.BodyParser(&req); err != nil {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "Invalid request body: " + err.Error(),
		})
	}

	if req.DownloadURL == "" {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{
			"error":   true,
			"message": "download_url is required",
		})
	}

	clientCfg := clients.TorrentClientConfig{
		ClientType: clients.TorrentClientType(h.cfg.TorrentClient.ClientType),
		BaseURL:    h.cfg.TorrentClient.BaseURL,
		Username:   h.cfg.TorrentClient.Username,
		Password:   h.cfg.TorrentClient.Password,
	}

	torrentClient, err := clients.NewTorrentClient(clientCfg)
	if err != nil {
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to create torrent client: " + err.Error(),
		})
	}

	if strings.HasPrefix(req.DownloadURL, "magnet:") {
		if err := torrentClient.AddTorrentURL(req.DownloadURL, true); err != nil {
			return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
				"error":   true,
				"message": "Failed to add magnet link: " + err.Error(),
			})
		}
	} else {
		// Download .torrent file bytes
		resp, err := http.Get(req.DownloadURL)
		if err != nil {
			return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
				"error":   true,
				"message": "Failed to download torrent file: " + err.Error(),
			})
		}
		defer resp.Body.Close()

		if resp.StatusCode != http.StatusOK {
			return c.Status(fiber.StatusBadGateway).JSON(fiber.Map{
				"error":   true,
				"message": fmt.Sprintf("Indexer returned status %d when fetching torrent", resp.StatusCode),
			})
		}

		torrentBytes, err := io.ReadAll(resp.Body)
		if err != nil {
			return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
				"error":   true,
				"message": "Failed to read torrent file: " + err.Error(),
			})
		}

		filename := req.Title
		if filename == "" {
			filename = "download"
		}
		filename += ".torrent"

		if err := torrentClient.AddTorrentFile(torrentBytes, filename, true); err != nil {
			return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
				"error":   true,
				"message": "Failed to add torrent file: " + err.Error(),
			})
		}
	}

	return c.JSON(fiber.Map{
		"success": true,
		"message": "Torrent added successfully",
	})
}

// TestIndexer tests connectivity to an indexer by running a blank search
func (h *IndexerHandler) TestIndexer(c *fiber.Ctx) error {
	id := c.Params("id")

	repo := db.NewIndexerRepository()
	indexer, err := repo.GetByID(id)
	if err != nil {
		if err == db.ErrIndexerNotFound {
			return c.Status(fiber.StatusNotFound).JSON(fiber.Map{
				"error":   true,
				"message": "Indexer not found",
			})
		}
		return c.Status(fiber.StatusInternalServerError).JSON(fiber.Map{
			"error":   true,
			"message": "Failed to retrieve indexer: " + err.Error(),
		})
	}

	client := clients.NewIndexerClient(indexer)
	results, err := client.Search(&models.IndexerSearchRequest{Limit: 5})
	if err != nil {
		return c.Status(fiber.StatusBadGateway).JSON(fiber.Map{
			"success": false,
			"error":   true,
			"message": "Indexer test failed: " + err.Error(),
		})
	}

	return c.JSON(fiber.Map{
		"success": true,
		"count":   len(results),
	})
}

// GetIndexerHealth handles GET /api/v1/indexers/health.
// It returns in-memory runtime stats for every registered built-in indexer.
func (h *IndexerHandler) GetIndexerHealth(c *fiber.Ctx) error {
	if h.registry == nil {
		return c.JSON(fiber.Map{"indexers": []indexers.IndexerStats{}})
	}
	return c.JSON(fiber.Map{"indexers": h.registry.Health()})
}
