package notifications

import "github.com/rs/zerolog/log"

// Registry holds a list of Notifiers and dispatches events to all of them.
type Registry struct {
	notifiers []Notifier
}

// NewRegistry creates a new empty Registry.
func NewRegistry() *Registry {
	return &Registry{}
}

// Register adds a Notifier to the registry.
func (r *Registry) Register(n Notifier) {
	r.notifiers = append(r.notifiers, n)
}

// Fire sends the given event to every registered Notifier, logging any errors.
func (r *Registry) Fire(event NotificationEvent) {
	for _, n := range r.notifiers {
		if err := n.Notify(event); err != nil {
			log.Error().
				Err(err).
				Str("notifier", n.Name()).
				Str("event_type", event.Type).
				Msg("Notification failed")
		}
	}
}
