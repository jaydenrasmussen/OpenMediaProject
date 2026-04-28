package logger

import (
	"context"
	"fmt"
	"io"
	"os"
	"strings"
	"time"

	"backend/config"

	"github.com/rs/zerolog"
	"github.com/rs/zerolog/log"
)

type contextKey string

// Predefined keys for log fields
const (
	KeyRequestID   = "request_id"
	KeyUserID      = "user_id"
	KeyMethod      = "method"
	KeyPath        = "path"
	KeyStatusCode  = "status"
	KeyLatency     = "latency"
	KeyUserAgent   = "user_agent"
	KeyClientIP    = "client_ip"
	KeyError       = "error"
	KeyStackTrace  = "stack_trace"
	KeyComponent   = "component"
	KeyAction      = "action"
	KeyResult      = "result"
	KeyContentType = "content_type"
	KeySize        = "size"
	KeyTimeout     = "timeout"
	KeyRetry       = "retry"

	// Context key for request-scoped logger
	LoggerCtxKey = contextKey("logger")
)

// LogLevel represents the logging level
type LogLevel string

// Supported log levels
const (
	DebugLevel LogLevel = "debug"
	InfoLevel  LogLevel = "info"
	WarnLevel  LogLevel = "warn"
	ErrorLevel LogLevel = "error"
)

var globalLogger zerolog.Logger

// Init initializes the global logger with the specified configuration
func Init(cfg *config.Config) {
	// Parse log level
	level := parseLogLevel(cfg.LogLevel)

	// Set global level
	zerolog.SetGlobalLevel(level)

	// Set time format
	zerolog.TimeFieldFormat = time.RFC3339

	// Create console writer with pretty formatting for development
	var output io.Writer = os.Stdout
	if cfg.Environment == "development" {
		output = zerolog.ConsoleWriter{
			Out:        os.Stdout,
			TimeFormat: "15:04:05",
			FormatLevel: func(i interface{}) string {
				if i == nil {
					return ""
				}
				level, ok := i.(string)
				if !ok {
					return fmt.Sprintf("%v", i)
				}
				switch level {
				case "debug":
					return "\033[36mDEBUG\033[0m"
				case "info":
					return "\033[32mINFO\033[0m "
				case "warn":
					return "\033[33mWARN\033[0m "
				case "error":
					return "\033[31mERROR\033[0m"
				default:
					return fmt.Sprintf("\033[37m%s\033[0m", strings.ToUpper(level))
				}
			},
		}
	}

	// Create logger
	logger := zerolog.New(output).With().
		Timestamp().
		Str("service", "openmedia").
		Str("env", cfg.Environment).
		Logger()

	// Set global logger
	globalLogger = logger
	log.Logger = logger
}

// parseLogLevel converts a string log level to zerolog.Level
func parseLogLevel(levelStr string) zerolog.Level {
	switch strings.ToLower(levelStr) {
	case "debug":
		return zerolog.DebugLevel
	case "info":
		return zerolog.InfoLevel
	case "warn":
		return zerolog.WarnLevel
	case "error":
		return zerolog.ErrorLevel
	default:
		return zerolog.InfoLevel
	}
}

// Debug logs a debug message
func Debug() *zerolog.Event {
	return globalLogger.Debug()
}

// Info logs an info message
func Info() *zerolog.Event {
	return globalLogger.Info()
}

// Warn logs a warning message
func Warn() *zerolog.Event {
	return globalLogger.Warn()
}

// Error logs an error message
func Error() *zerolog.Event {
	return globalLogger.Error()
}

// Fatal logs a fatal message and exits the program
func Fatal() *zerolog.Event {
	return globalLogger.Fatal()
}

// WithRequestID adds a request ID to the logger
func WithRequestID(requestID string) zerolog.Logger {
	return globalLogger.With().Str(KeyRequestID, requestID).Logger()
}

// WithComponent adds a component name to the logger
func WithComponent(component string) zerolog.Logger {
	return globalLogger.With().Str(KeyComponent, component).Logger()
}

// WithError adds an error to the logger
func WithError(err error) zerolog.Logger {
	return globalLogger.With().Err(err).Logger()
}

// WithContext extracts the logger from the context if present, or returns the global logger
func WithContext(ctx context.Context) zerolog.Logger {
	if ctx == nil {
		return globalLogger
	}
	if l, ok := ctx.Value(LoggerCtxKey).(zerolog.Logger); ok {
		return l
	}
	return globalLogger
}

// ContextWithLogger adds a logger to the context
func ContextWithLogger(ctx context.Context, logger zerolog.Logger) context.Context {
	return context.WithValue(ctx, LoggerCtxKey, logger)
}

// SetLogLevel updates the global log level
func SetLogLevel(level LogLevel) {
	zerolog.SetGlobalLevel(parseLogLevel(string(level)))
}
