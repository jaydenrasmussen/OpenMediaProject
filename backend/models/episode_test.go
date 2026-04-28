package models

import (
	"encoding/json"
	"strings"
	"testing"
)

// ── NewEpisodeFromTVDBData ────────────────────────────────────────────────────

func TestNewEpisodeFromTVDBData_ValidFull(t *testing.T) {
	raw := map[string]interface{}{
		"id":           float64(12345),
		"name":         "Pilot",
		"aired":        "2008-01-20",
		"seasonNumber": float64(1),
		"number":       float64(1),
		"overview":     "The beginning.",
	}
	ep := NewEpisodeFromTVDBData("show-1", raw)
	if ep == nil {
		t.Fatal("NewEpisodeFromTVDBData returned nil for valid data")
	}
	if ep.ID != "12345" {
		t.Errorf("ID = %q, want %q", ep.ID, "12345")
	}
	if ep.ShowID != "show-1" {
		t.Errorf("ShowID = %q, want %q", ep.ShowID, "show-1")
	}
	if ep.Title != "Pilot" {
		t.Errorf("Title = %q, want %q", ep.Title, "Pilot")
	}
	if ep.AirDate != "2008-01-20" {
		t.Errorf("AirDate = %q, want %q", ep.AirDate, "2008-01-20")
	}
	if ep.Season != 1 {
		t.Errorf("Season = %d, want 1", ep.Season)
	}
	if ep.Number != 1 {
		t.Errorf("Number = %d, want 1", ep.Number)
	}
	if ep.Overview != "The beginning." {
		t.Errorf("Overview = %q, want %q", ep.Overview, "The beginning.")
	}
}

func TestNewEpisodeFromTVDBData_MissingID(t *testing.T) {
	raw := map[string]interface{}{
		"name": "No ID Episode",
	}
	ep := NewEpisodeFromTVDBData("show-1", raw)
	if ep != nil {
		t.Errorf("expected nil for missing id, got %+v", ep)
	}
}

func TestNewEpisodeFromTVDBData_ZeroID(t *testing.T) {
	raw := map[string]interface{}{
		"id":   float64(0),
		"name": "Zero ID Episode",
	}
	ep := NewEpisodeFromTVDBData("show-1", raw)
	if ep != nil {
		t.Errorf("expected nil for id=0, got %+v", ep)
	}
}

func TestNewEpisodeFromTVDBData_WrongIDType(t *testing.T) {
	raw := map[string]interface{}{
		"id":   "not-a-float",
		"name": "String ID",
	}
	ep := NewEpisodeFromTVDBData("show-1", raw)
	if ep != nil {
		t.Errorf("expected nil for string id, got %+v", ep)
	}
}

func TestNewEpisodeFromTVDBData_MissingOptionalFields(t *testing.T) {
	// Only id provided; all other fields should have zero values.
	raw := map[string]interface{}{
		"id": float64(999),
	}
	ep := NewEpisodeFromTVDBData("show-2", raw)
	if ep == nil {
		t.Fatal("expected non-nil episode for valid id")
	}
	if ep.Title != "" {
		t.Errorf("Title should be empty, got %q", ep.Title)
	}
	if ep.Season != 0 {
		t.Errorf("Season should be 0, got %d", ep.Season)
	}
}

// ── EpisodeStatus constants ───────────────────────────────────────────────────

func TestEpisodeStatusConstants(t *testing.T) {
	if EpisodeStatusWanted != "wanted" {
		t.Errorf("EpisodeStatusWanted = %q", EpisodeStatusWanted)
	}
	if EpisodeStatusDownloading != "downloading" {
		t.Errorf("EpisodeStatusDownloading = %q", EpisodeStatusDownloading)
	}
	if EpisodeStatusDownloaded != "downloaded" {
		t.Errorf("EpisodeStatusDownloaded = %q", EpisodeStatusDownloaded)
	}
	if EpisodeStatusSkipped != "skipped" {
		t.Errorf("EpisodeStatusSkipped = %q", EpisodeStatusSkipped)
	}
	if EpisodeStatusMissing != "missing" {
		t.Errorf("EpisodeStatusMissing = %q", EpisodeStatusMissing)
	}
}

// ── Episode JSON round-trip ───────────────────────────────────────────────────

func TestEpisode_JSONRoundTrip(t *testing.T) {
	ep := &Episode{
		ID:      "42",
		ShowID:  "show-1",
		Season:  2,
		Number:  3,
		Title:   "The One",
		AirDate: "2024-03-15",
		Status:  EpisodeStatusDownloaded,
		Quality: "1080p",
	}

	data, err := json.Marshal(ep)
	if err != nil {
		t.Fatalf("json.Marshal error = %v", err)
	}

	var got Episode
	if err := json.Unmarshal(data, &got); err != nil {
		t.Fatalf("json.Unmarshal error = %v", err)
	}

	if got.ID != ep.ID {
		t.Errorf("ID: got %q, want %q", got.ID, ep.ID)
	}
	if got.Season != ep.Season {
		t.Errorf("Season: got %d, want %d", got.Season, ep.Season)
	}
	if got.Status != ep.Status {
		t.Errorf("Status: got %q, want %q", got.Status, ep.Status)
	}
}

// ── Movie JSON ────────────────────────────────────────────────────────────────

func TestMovie_ToJSON(t *testing.T) {
	m := &Movie{ID: "1", Title: "Inception", Rating: 8.8}
	data, err := m.ToJSON()
	if err != nil {
		t.Fatalf("ToJSON error = %v", err)
	}
	if !strings.Contains(string(data), "Inception") {
		t.Errorf("JSON does not contain title: %s", data)
	}
}

func TestMovie_FromJSON(t *testing.T) {
	raw := `{"id":"1","title":"Inception","rating":8.8}`
	m := &Movie{}
	if err := m.FromJSON([]byte(raw)); err != nil {
		t.Fatalf("FromJSON error = %v", err)
	}
	if m.Title != "Inception" {
		t.Errorf("Title = %q, want %q", m.Title, "Inception")
	}
	if m.Rating != 8.8 {
		t.Errorf("Rating = %f, want 8.8", m.Rating)
	}
}

func TestMovie_FromJSON_Invalid(t *testing.T) {
	m := &Movie{}
	if err := m.FromJSON([]byte("not json")); err == nil {
		t.Error("expected error for invalid JSON, got nil")
	}
}
