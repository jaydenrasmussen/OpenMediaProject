package clients

// MediaServer is the interface every media server implementation must satisfy.
type MediaServer interface {
	// Name returns a human-readable identifier for logging.
	Name() string
	// ScanLibrary triggers a library refresh on the media server.
	ScanLibrary() error
}
