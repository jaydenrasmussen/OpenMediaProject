package api

import (
	"testing"
	"time"

	"backend/models"
)

// episodeStatusFromAirDate is a pure function — no DB or HTTP needed.

func TestEpisodeStatusFromAirDate_Empty(t *testing.T) {
	today := time.Now().Truncate(24 * time.Hour)
	got := episodeStatusFromAirDate("", today)
	if got != models.EpisodeStatusMissing {
		t.Errorf("got %q, want %q (empty air date)", got, models.EpisodeStatusMissing)
	}
}

func TestEpisodeStatusFromAirDate_Unparseable(t *testing.T) {
	today := time.Now().Truncate(24 * time.Hour)
	got := episodeStatusFromAirDate("not-a-date", today)
	if got != models.EpisodeStatusMissing {
		t.Errorf("got %q, want %q (unparseable date)", got, models.EpisodeStatusMissing)
	}
}

func TestEpisodeStatusFromAirDate_FutureDate(t *testing.T) {
	today := time.Now().Truncate(24 * time.Hour)
	// Use a date far enough in the future that timezone differences can't cause it
	// to appear as today or past.
	const farFuture = "2099-12-31"
	got := episodeStatusFromAirDate(farFuture, today)
	if got != models.EpisodeStatusWanted {
		t.Errorf("got %q, want %q (far-future air date %q)", got, models.EpisodeStatusWanted, farFuture)
	}
}

func TestEpisodeStatusFromAirDate_PastDate(t *testing.T) {
	today := time.Now().Truncate(24 * time.Hour)
	// yesterday
	past := today.Add(-24 * time.Hour).Format("2006-01-02")
	got := episodeStatusFromAirDate(past, today)
	if got != models.EpisodeStatusMissing {
		t.Errorf("got %q, want %q (past air date)", got, models.EpisodeStatusMissing)
	}
}

func TestEpisodeStatusFromAirDate_Today(t *testing.T) {
	today := time.Now().Truncate(24 * time.Hour)
	got := episodeStatusFromAirDate(today.Format("2006-01-02"), today)
	// Today is NOT after today → missing (already should have aired)
	if got != models.EpisodeStatusMissing {
		t.Errorf("got %q, want %q (today's date)", got, models.EpisodeStatusMissing)
	}
}
