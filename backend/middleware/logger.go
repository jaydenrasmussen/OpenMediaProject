package middleware

import (
	"fmt"
	"time"

	"backend/logger"

	"github.com/gofiber/fiber/v2"
	"github.com/google/uuid"
	"github.com/rs/zerolog"
	"github.com/rs/zerolog/log"
)

// ZeroLogger creates a middleware that logs HTTP requests using zerolog
func ZeroLogger() fiber.Handler {
	return func(c *fiber.Ctx) error {
		// Start timer
		start := time.Now()

		// Generate a request ID if not present
		requestID := c.Get(fiber.HeaderXRequestID)
		if requestID == "" {
			requestID = uuid.New().String()
			c.Set(fiber.HeaderXRequestID, requestID)
		}

		// Get route and method
		path := c.Path()
		method := c.Method()

		// Create request-scoped logger
		reqLogger := log.With().
			Str(logger.KeyRequestID, requestID).
			Str(logger.KeyMethod, method).
			Str(logger.KeyPath, path).
			Str(logger.KeyClientIP, c.IP()).
			Logger()

		// Log request start
		reqLogger.Debug().
			Str(logger.KeyUserAgent, c.Get(fiber.HeaderUserAgent)).
			Msg("Request received")

		// Store logger in context (local context, not Go context)
		c.Locals("logger", reqLogger)

		// Process request
		err := c.Next()

		// Get response status and latency
		status := c.Response().StatusCode()
		latency := time.Since(start)

		// Determine log level based on status code
		logEvent := reqLogger.Info()
		if status >= 500 {
			logEvent = reqLogger.Error()
		} else if status >= 400 {
			logEvent = reqLogger.Warn()
		}

		// Log request completion
		logEvent.
			Int(logger.KeyStatusCode, status).
			Str(logger.KeyLatency, fmt.Sprintf("%s", latency)).
			Str(logger.KeyContentType, c.Get(fiber.HeaderContentType)).
			Int(logger.KeySize, len(c.Response().Body())).
			Msg("Request completed")

		return err
	}
}

// GetRequestLogger extracts the request-scoped logger from fiber context
func GetRequestLogger(c *fiber.Ctx) zerolog.Logger {
	if loggerVal, ok := c.Locals("logger").(zerolog.Logger); ok {
		return loggerVal
	}
	// Fallback to global logger with request ID if available
	requestID := c.Get(fiber.HeaderXRequestID)
	if requestID != "" {
		return log.With().Str(logger.KeyRequestID, requestID).Logger()
	}
	return log.Logger
}
