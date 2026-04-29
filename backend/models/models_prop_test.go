package models

import (
	"fmt"
	"testing"

	"pgregory.net/rapid"
)

// scaledFloat generates a float64 in [lo, hi] using integer steps so that
// the result is always finite and JSON-serialisable (no NaN / Inf).
func scaledFloat(t *rapid.T, name string, lo, hi float64) float64 {
	n := rapid.IntRange(0, 10_000).Draw(t, name)
	return lo + float64(n)*(hi-lo)/10_000.0
}

// safeStringSlice returns a non-nil slice so that nil vs [] does not produce
// spurious failures when comparing JSON round-tripped genre lists.
func safeStringSlice(t *rapid.T, name string) []string {
	s := rapid.SliceOf(rapid.String()).Draw(t, name)
	if s == nil {
		return []string{}
	}
	return s
}

// ── User ──────────────────────────────────────────────────────────────────────

// Property: User.ToJSON → User.FromJSON round-trip preserves all exported fields.
func TestUser_JSONRoundTripProp(t *testing.T) {
	roles := []UserRole{UserRoleAdmin, UserRoleUser}
	rapid.Check(t, func(t *rapid.T) {
		u := &User{
			ID:       rapid.String().Draw(t, "id"),
			Username: rapid.String().Draw(t, "username"),
			Email:    rapid.String().Draw(t, "email"),
			Role:     rapid.SampledFrom(roles).Draw(t, "role"),
			IsActive: rapid.Bool().Draw(t, "is_active"),
		}
		data, err := u.ToJSON()
		if err != nil {
			t.Fatalf("ToJSON error = %v", err)
		}
		var got User
		if err := got.FromJSON(data); err != nil {
			t.Fatalf("FromJSON error = %v", err)
		}
		if got.ID != u.ID {
			t.Fatalf("ID: got %q, want %q", got.ID, u.ID)
		}
		if got.Username != u.Username {
			t.Fatalf("Username: got %q, want %q", got.Username, u.Username)
		}
		if got.Email != u.Email {
			t.Fatalf("Email: got %q, want %q", got.Email, u.Email)
		}
		if got.Role != u.Role {
			t.Fatalf("Role: got %q, want %q", got.Role, u.Role)
		}
		if got.IsActive != u.IsActive {
			t.Fatalf("IsActive: got %v, want %v", got.IsActive, u.IsActive)
		}
	})
}

// Property: BeforeCreate always sets IsActive=true, a non-empty Role, and
// CreatedAt == UpdatedAt regardless of the initial role value.
func TestUser_BeforeCreateProp(t *testing.T) {
	roleInputs := []string{"", string(UserRoleAdmin), string(UserRoleUser)}
	rapid.Check(t, func(t *rapid.T) {
		u := &User{
			ID:       rapid.String().Draw(t, "id"),
			Username: rapid.String().Draw(t, "username"),
			Role:     UserRole(rapid.SampledFrom(roleInputs).Draw(t, "role")),
		}
		u.BeforeCreate()
		if !u.IsActive {
			t.Fatal("BeforeCreate must set IsActive = true")
		}
		if u.Role == "" {
			t.Fatal("BeforeCreate must set a non-empty Role")
		}
		if !u.CreatedAt.Equal(u.UpdatedAt) {
			t.Fatalf("BeforeCreate: CreatedAt (%v) != UpdatedAt (%v)", u.CreatedAt, u.UpdatedAt)
		}
	})
}

// Property: GetKey returns the ID encoded as bytes for any ID string.
func TestUser_GetKeyProp(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		id := rapid.String().Draw(t, "id")
		u := &User{ID: id}
		if got := string(u.GetKey()); got != id {
			t.Fatalf("GetKey() = %q, want %q", got, id)
		}
	})
}

// ── TVShow ────────────────────────────────────────────────────────────────────

// Property: TVShow.ToJSON → TVShow.FromJSON round-trip preserves key fields.
func TestTVShow_JSONRoundTripProp(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		s := &TVShow{
			ID:     rapid.String().Draw(t, "id"),
			Name:   rapid.String().Draw(t, "name"),
			Rating: scaledFloat(t, "rating", 0, 10),
			Genres: safeStringSlice(t, "genres"),
		}
		data, err := s.ToJSON()
		if err != nil {
			t.Fatalf("ToJSON error = %v", err)
		}
		var got TVShow
		if err := got.FromJSON(data); err != nil {
			t.Fatalf("FromJSON error = %v", err)
		}
		if got.ID != s.ID {
			t.Fatalf("ID: got %q, want %q", got.ID, s.ID)
		}
		if got.Name != s.Name {
			t.Fatalf("Name: got %q, want %q", got.Name, s.Name)
		}
		if got.Rating != s.Rating {
			t.Fatalf("Rating: got %f, want %f", got.Rating, s.Rating)
		}
		if len(got.Genres) != len(s.Genres) {
			t.Fatalf("Genres len: got %d, want %d", len(got.Genres), len(s.Genres))
		}
	})
}

// Property: TVShow.GetKey returns []byte(ID) for any ID string.
func TestTVShow_GetKeyProp(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		id := rapid.String().Draw(t, "id")
		s := &TVShow{ID: id}
		if got := string(s.GetKey()); got != id {
			t.Fatalf("GetKey() = %q, want %q", got, id)
		}
	})
}

// Property: NewTVShowFromTVDBData never returns nil for any input map shape.
func TestNewTVShowFromTVDBData_NeverNilProp(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		data := map[string]interface{}{}
		switch rapid.IntRange(0, 2).Draw(t, "id_type") {
		case 0: // no id key
		case 1:
			data["id"] = rapid.String().Draw(t, "id_str")
		case 2:
			data["id"] = float64(rapid.IntRange(0, 1_000_000).Draw(t, "id_float"))
		}
		if show := NewTVShowFromTVDBData(data); show == nil {
			t.Fatal("NewTVShowFromTVDBData must never return nil")
		}
	})
}

// Property: a string "id" in the map is preserved verbatim in TVShow.ID.
func TestNewTVShowFromTVDBData_StringIDPreservedProp(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		id := rapid.String().Draw(t, "id")
		show := NewTVShowFromTVDBData(map[string]interface{}{"id": id})
		if show.ID != id {
			t.Fatalf("ID: got %q, want %q", show.ID, id)
		}
	})
}

// Property: a positive float64 "id" is formatted as its integer string.
func TestNewTVShowFromTVDBData_Float64IDFormattedProp(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		n := rapid.IntRange(1, 1_000_000).Draw(t, "n")
		id := float64(n)
		show := NewTVShowFromTVDBData(map[string]interface{}{"id": id})
		want := fmt.Sprintf("%.0f", id)
		if show.ID != want {
			t.Fatalf("ID: got %q, want %q (from float64 %f)", show.ID, want, id)
		}
	})
}

// ── Movie ─────────────────────────────────────────────────────────────────────

// Property: Movie.ToJSON → Movie.FromJSON round-trip preserves key fields.
func TestMovie_JSONRoundTripProp(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		m := &Movie{
			ID:     rapid.String().Draw(t, "id"),
			Title:  rapid.String().Draw(t, "title"),
			Rating: scaledFloat(t, "rating", 0, 10),
			Genres: safeStringSlice(t, "genres"),
		}
		data, err := m.ToJSON()
		if err != nil {
			t.Fatalf("ToJSON error = %v", err)
		}
		var got Movie
		if err := got.FromJSON(data); err != nil {
			t.Fatalf("FromJSON error = %v", err)
		}
		if got.ID != m.ID {
			t.Fatalf("ID: got %q, want %q", got.ID, m.ID)
		}
		if got.Title != m.Title {
			t.Fatalf("Title: got %q, want %q", got.Title, m.Title)
		}
		if got.Rating != m.Rating {
			t.Fatalf("Rating: got %f, want %f", got.Rating, m.Rating)
		}
	})
}

// Property: Movie.GetKey returns []byte(ID) for any ID string.
func TestMovie_GetKeyProp(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		id := rapid.String().Draw(t, "id")
		m := &Movie{ID: id}
		if got := string(m.GetKey()); got != id {
			t.Fatalf("GetKey() = %q, want %q", got, id)
		}
	})
}

// ── Episode ───────────────────────────────────────────────────────────────────

// Property: any positive float64 id produces a non-nil episode.
func TestNewEpisodeFromTVDBData_PositiveIDNeverNilProp(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		id := float64(rapid.IntRange(1, 1_000_000).Draw(t, "id"))
		showID := rapid.String().Draw(t, "show_id")
		ep := NewEpisodeFromTVDBData(showID, map[string]interface{}{"id": id})
		if ep == nil {
			t.Fatalf("NewEpisodeFromTVDBData(id=%f) returned nil", id)
		}
	})
}

// Property: ShowID is always the value passed to NewEpisodeFromTVDBData.
func TestNewEpisodeFromTVDBData_ShowIDPreservedProp(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		showID := rapid.String().Draw(t, "show_id")
		id := float64(rapid.IntRange(1, 1_000_000).Draw(t, "id"))
		ep := NewEpisodeFromTVDBData(showID, map[string]interface{}{"id": id})
		if ep.ShowID != showID {
			t.Fatalf("ShowID: got %q, want %q", ep.ShowID, showID)
		}
	})
}

// Property: Episode.ID is always the integer-string form of the float64 id.
func TestNewEpisodeFromTVDBData_IDFormattedProp(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		n := rapid.IntRange(1, 1_000_000).Draw(t, "n")
		id := float64(n)
		ep := NewEpisodeFromTVDBData("show-1", map[string]interface{}{"id": id})
		want := fmt.Sprintf("%.0f", id)
		if ep.ID != want {
			t.Fatalf("ID: got %q, want %q (from float64 %f)", ep.ID, want, id)
		}
	})
}
