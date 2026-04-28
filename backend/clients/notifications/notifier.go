package notifications

// NotificationEvent carries the data for a single notification.
type NotificationEvent struct {
	Type    string // "download_complete" | "download_failed" | "new_request"
	Title   string
	Message string
	User    string
}

// Notifier is the interface that all notification backends must implement.
type Notifier interface {
	Name() string
	Notify(event NotificationEvent) error
}
