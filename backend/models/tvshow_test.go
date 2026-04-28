package models

import (
	"strings"
	"testing"
)

// ── TVShow.ToJSON / FromJSON ───────────────────────────────────────────────────

func TestTVShow_ToJSON(t *testing.T) {
	s := &TVShow{ID: "12345", Name: "Breaking Bad", Rating: 9.5}
	data, err := s.ToJSON()
	if err != nil {
		t.Fatalf("ToJSON error = %v", err)
	}
	if !strings.Contains(string(data), "Breaking Bad") {
		t.Errorf("JSON does not contain show name: %s", data)
	}
}

func TestTVShow_FromJSON(t *testing.T) {
	raw := `{"id":"99","name":"The Wire","rating":9.3,"genres":["Drama"]}`
	s := &TVShow{}
	if err := s.FromJSON([]byte(raw)); err != nil {
		t.Fatalf("FromJSON error = %v", err)
	}
	if s.Name != "The Wire" {
		t.Errorf("Name = %q, want %q", s.Name, "The Wire")
	}
	if s.Rating != 9.3 {
		t.Errorf("Rating = %f, want 9.3", s.Rating)
	}
}

func TestTVShow_FromJSON_Invalid(t *testing.T) {
	s := &TVShow{}
	if err := s.FromJSON([]byte("not json")); err == nil {
		t.Error("expected error for invalid JSON, got nil")
	}
}

// ── TVShow.GetKey ─────────────────────────────────────────────────────────────

func TestTVShow_GetKey(t *testing.T) {
	s := &TVShow{ID: "show-42"}
	key := s.GetKey()
	if string(key) != "show-42" {
		t.Errorf("GetKey() = %q, want %q", key, "show-42")
	}
}

// ── NewTVShowFromTVDBData ─────────────────────────────────────────────────────

func TestNewTVShowFromTVDBData_StringID(t *testing.T) {
	data := map[string]interface{}{
		"id":         "12345",
		"name":       "Breaking Bad",
		"overview":   "A chemistry teacher turns drug lord.",
		"firstAired": "2008-01-20",
		"network":    "AMC",
		"status":     "Ended",
		"poster":     "http://example.com/poster.jpg",
		"backdrop":   "http://example.com/backdrop.jpg",
		"rating":     float64(9.5),
		"genres":     []interface{}{"Drama", "Thriller"},
	}
	show := NewTVShowFromTVDBData(data)
	if show == nil {
		t.Fatal("NewTVShowFromTVDBData returned nil")
	}
	if show.ID != "12345" {
		t.Errorf("ID = %q, want %q", show.ID, "12345")
	}
	if show.Name != "Breaking Bad" {
		t.Errorf("Name = %q, want %q", show.Name, "Breaking Bad")
	}
	if show.Network != "AMC" {
		t.Errorf("Network = %q, want %q", show.Network, "AMC")
	}
	if show.Rating != 9.5 {
		t.Errorf("Rating = %f, want 9.5", show.Rating)
	}
	if len(show.Genres) != 2 {
		t.Errorf("Genres len = %d, want 2", len(show.Genres))
	}
}

func TestNewTVShowFromTVDBData_FloatID(t *testing.T) {
	data := map[string]interface{}{
		"id":   float64(99999),
		"name": "The Sopranos",
	}
	show := NewTVShowFromTVDBData(data)
	if show.ID != "99999" {
		t.Errorf("ID = %q, want %q", show.ID, "99999")
	}
}

func TestNewTVShowFromTVDBData_StringRating(t *testing.T) {
	data := map[string]interface{}{
		"id":     "1",
		"name":   "Show",
		"rating": "8.7",
	}
	show := NewTVShowFromTVDBData(data)
	if show.Rating != 8.7 {
		t.Errorf("Rating = %f, want 8.7 (parsed from string)", show.Rating)
	}
}

func TestNewTVShowFromTVDBData_ImageFallback(t *testing.T) {
	// When "poster" is missing, should fall back to "image" field.
	data := map[string]interface{}{
		"id":    "1",
		"name":  "Show",
		"image": "http://example.com/image.jpg",
	}
	show := NewTVShowFromTVDBData(data)
	if show.PosterURL != "http://example.com/image.jpg" {
		t.Errorf("PosterURL = %q, want image fallback", show.PosterURL)
	}
}

func TestNewTVShowFromTVDBData_EmptyMap(t *testing.T) {
	show := NewTVShowFromTVDBData(map[string]interface{}{})
	if show == nil {
		t.Fatal("NewTVShowFromTVDBData returned nil for empty map")
	}
	if show.ID != "" {
		t.Errorf("ID should be empty for missing id, got %q", show.ID)
	}
}

// ── Movie.GetKey ──────────────────────────────────────────────────────────────

func TestMovie_GetKey(t *testing.T) {
	m := &Movie{ID: "movie-123"}
	key := m.GetKey()
	if string(key) != "movie-123" {
		t.Errorf("GetKey() = %q, want %q", key, "movie-123")
	}
}
