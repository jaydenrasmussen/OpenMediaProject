package clients

import (
	"testing"

	"backend/models"
)

// ── sanitise ──────────────────────────────────────────────────────────────────

func TestSanitise_NoIllegalChars(t *testing.T) {
	input := "Breaking Bad"
	got := sanitise(input)
	if got != "Breaking Bad" {
		t.Errorf("sanitise(%q) = %q, want %q", input, got, "Breaking Bad")
	}
}

func TestSanitise_ReplacesForwardSlash(t *testing.T) {
	got := sanitise("AC/DC")
	if got != "AC-DC" {
		t.Errorf("sanitise(\"AC/DC\") = %q, want %q", got, "AC-DC")
	}
}

func TestSanitise_ReplacesColon(t *testing.T) {
	got := sanitise("Star Wars: A New Hope")
	if got != "Star Wars- A New Hope" {
		t.Errorf("sanitise(%q) = %q", "Star Wars: A New Hope", got)
	}
}

func TestSanitise_TrimsSpace(t *testing.T) {
	got := sanitise("  trimmed  ")
	if got != "trimmed" {
		t.Errorf("sanitise(\"  trimmed  \") = %q, want %q", got, "trimmed")
	}
}

func TestSanitise_ReplacesMultipleIllegalChars(t *testing.T) {
	// All of / \ : * ? " < > | should become -
	got := sanitise(`/\:*?"<>|`)
	for _, r := range got {
		if r != '-' {
			t.Errorf("expected all chars to become '-', got char %q in %q", r, got)
			break
		}
	}
}

func TestSanitise_EmptyString(t *testing.T) {
	got := sanitise("")
	if got != "" {
		t.Errorf("sanitise(\"\") = %q, want empty", got)
	}
}

// ── movieYear ─────────────────────────────────────────────────────────────────

func TestMovieYear_ValidDate(t *testing.T) {
	m := &models.Movie{ReleaseDate: "2024-07-15"}
	if got := movieYear(m); got != "2024" {
		t.Errorf("movieYear = %q, want %q", got, "2024")
	}
}

func TestMovieYear_EmptyDate(t *testing.T) {
	m := &models.Movie{ReleaseDate: ""}
	if got := movieYear(m); got != "" {
		t.Errorf("movieYear = %q, want empty", got)
	}
}

func TestMovieYear_ShortDate(t *testing.T) {
	m := &models.Movie{ReleaseDate: "202"} // only 3 chars
	if got := movieYear(m); got != "" {
		t.Errorf("movieYear = %q, want empty for short date", got)
	}
}

func TestMovieYear_ExactlyFourChars(t *testing.T) {
	m := &models.Movie{ReleaseDate: "1999"}
	if got := movieYear(m); got != "1999" {
		t.Errorf("movieYear = %q, want %q", got, "1999")
	}
}

// ── movieFolderName ───────────────────────────────────────────────────────────

func TestMovieFolderName_WithYear(t *testing.T) {
	m := &models.Movie{Title: "Inception", ReleaseDate: "2010-07-16"}
	got := movieFolderName(m)
	if got != "Inception (2010)" {
		t.Errorf("movieFolderName = %q, want %q", got, "Inception (2010)")
	}
}

func TestMovieFolderName_WithoutYear(t *testing.T) {
	m := &models.Movie{Title: "Unknown Movie", ReleaseDate: ""}
	got := movieFolderName(m)
	if got != "Unknown Movie" {
		t.Errorf("movieFolderName = %q, want %q", got, "Unknown Movie")
	}
}

func TestMovieFolderName_SanitisesTitle(t *testing.T) {
	m := &models.Movie{Title: "Star Wars: A New Hope", ReleaseDate: "1977-05-25"}
	got := movieFolderName(m)
	// Colon should be replaced.
	if got == "Star Wars: A New Hope (1977)" {
		t.Errorf("movieFolderName should sanitise colon in title, got %q", got)
	}
}

// ── tvBaseName ────────────────────────────────────────────────────────────────

func TestTVBaseName(t *testing.T) {
	show := &models.TVShow{Name: "Breaking Bad"}
	ep := &models.Episode{Season: 1, Number: 1, Title: "Pilot"}
	got := tvBaseName(show, ep)
	want := "Breaking Bad - S01E01 - Pilot"
	if got != want {
		t.Errorf("tvBaseName = %q, want %q", got, want)
	}
}

func TestTVBaseName_PaddingSeasonAndEpisode(t *testing.T) {
	show := &models.TVShow{Name: "The Wire"}
	ep := &models.Episode{Season: 3, Number: 9, Title: "Slapstick"}
	got := tvBaseName(show, ep)
	want := "The Wire - S03E09 - Slapstick"
	if got != want {
		t.Errorf("tvBaseName = %q, want %q", got, want)
	}
}

func TestTVBaseName_SanitisesShowName(t *testing.T) {
	show := &models.TVShow{Name: "Show: The Series"}
	ep := &models.Episode{Season: 1, Number: 1, Title: "Intro"}
	got := tvBaseName(show, ep)
	// Show name should have colon replaced.
	if got == "Show: The Series - S01E01 - Intro" {
		t.Errorf("tvBaseName should sanitise colon in show name, got %q", got)
	}
}

// ── NewFileMover ──────────────────────────────────────────────────────────────

func TestNewFileMover(t *testing.T) {
	fm := NewFileMover("/movies", "/tvshows")
	if fm == nil {
		t.Fatal("NewFileMover returned nil")
	}
	if fm.movieDir != "/movies" {
		t.Errorf("movieDir = %q, want %q", fm.movieDir, "/movies")
	}
	if fm.tvShowDir != "/tvshows" {
		t.Errorf("tvShowDir = %q, want %q", fm.tvShowDir, "/tvshows")
	}
}

// ── tvSeasonDir ───────────────────────────────────────────────────────────────

func TestTVSeasonDir(t *testing.T) {
	fm := NewFileMover("/movies", "/tv")
	show := &models.TVShow{Name: "The Wire"}
	ep := &models.Episode{Season: 3}
	got := fm.tvSeasonDir(show, ep)

	// Should contain: tvShowDir / show name / Season NN
	if got == "" {
		t.Error("tvSeasonDir returned empty string")
	}
}
