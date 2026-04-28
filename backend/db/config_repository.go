package db

import (
	"encoding/json"

	"github.com/xujiajun/nutsdb"
)

const configKeySettings = "settings"

// ConfigRepository provides persistent key-value storage for application settings.
type ConfigRepository struct {
	db *nutsdb.DB
}

// NewConfigRepository creates a new ConfigRepository.
func NewConfigRepository() *ConfigRepository {
	return &ConfigRepository{db: GetDB()}
}

// GetSettings loads the persisted settings map. Returns an empty map if not set yet.
func (r *ConfigRepository) GetSettings() (map[string]interface{}, error) {
	out := make(map[string]interface{})

	err := r.db.View(func(tx *nutsdb.Tx) error {
		entry, err := tx.Get(ConfigBucket, []byte(configKeySettings))
		if err != nil {
			if err == nutsdb.ErrKeyNotFound {
				return nil // not set yet — return empty map
			}
			return err
		}
		return json.Unmarshal(entry.Value, &out)
	})

	return out, err
}

// SaveSettings persists the settings map, replacing any previous value.
func (r *ConfigRepository) SaveSettings(settings map[string]interface{}) error {
	data, err := json.Marshal(settings)
	if err != nil {
		return err
	}
	return r.db.Update(func(tx *nutsdb.Tx) error {
		return tx.Put(ConfigBucket, []byte(configKeySettings), data, 0)
	})
}
