package indexers

import (
	"errors"
	"testing"
)

// ── mock indexer ──────────────────────────────────────────────────────────────

type mockIndexer struct {
	name     string
	supports MediaType
	priority int
	enabled  bool
	results  []*SearchResult
	err      error
}

func (m *mockIndexer) Name() string                              { return m.name }
func (m *mockIndexer) Supports() MediaType                      { return m.supports }
func (m *mockIndexer) Priority() int                            { return m.priority }
func (m *mockIndexer) Enabled() bool                            { return m.enabled }
func (m *mockIndexer) SetEnabled(b bool)                        { m.enabled = b }
func (m *mockIndexer) Search(_ *SearchRequest) ([]*SearchResult, error) {
	return m.results, m.err
}

// ── Registry ──────────────────────────────────────────────────────────────────

func TestRegistry_Register_And_All(t *testing.T) {
	r := &Registry{}
	idx := &mockIndexer{name: "yts", enabled: true, supports: MediaTypeMovie, priority: 1}
	r.Register(idx)

	all := r.All()
	if len(all) != 1 {
		t.Fatalf("All() len = %d, want 1", len(all))
	}
	if all[0].Name() != "yts" {
		t.Errorf("all[0].Name() = %q, want %q", all[0].Name(), "yts")
	}
}

func TestRegistry_All_ReturnsSnapshot(t *testing.T) {
	r := &Registry{}
	r.Register(&mockIndexer{name: "a", enabled: true, supports: MediaTypeBoth, priority: 1})
	r.Register(&mockIndexer{name: "b", enabled: false, supports: MediaTypeBoth, priority: 2})

	all := r.All()
	if len(all) != 2 {
		t.Fatalf("All() len = %d, want 2", len(all))
	}
}

func TestRegistry_Enabled_FiltersDisabled(t *testing.T) {
	r := &Registry{}
	r.Register(&mockIndexer{name: "on", enabled: true, supports: MediaTypeBoth, priority: 1})
	r.Register(&mockIndexer{name: "off", enabled: false, supports: MediaTypeBoth, priority: 2})

	enabled := r.Enabled()
	if len(enabled) != 1 {
		t.Fatalf("Enabled() len = %d, want 1", len(enabled))
	}
	if enabled[0].Name() != "on" {
		t.Errorf("Enabled()[0].Name() = %q, want %q", enabled[0].Name(), "on")
	}
}

func TestRegistry_Enabled_AllEnabled(t *testing.T) {
	r := &Registry{}
	r.Register(&mockIndexer{name: "a", enabled: true, supports: MediaTypeBoth, priority: 1})
	r.Register(&mockIndexer{name: "b", enabled: true, supports: MediaTypeBoth, priority: 2})

	if got := len(r.Enabled()); got != 2 {
		t.Errorf("Enabled() len = %d, want 2", got)
	}
}

func TestRegistry_Enabled_NoneEnabled(t *testing.T) {
	r := &Registry{}
	r.Register(&mockIndexer{name: "off", enabled: false, supports: MediaTypeBoth, priority: 1})

	if got := len(r.Enabled()); got != 0 {
		t.Errorf("Enabled() len = %d, want 0", got)
	}
}

// ── supportsMediaType ─────────────────────────────────────────────────────────

func TestSupportsMediaType(t *testing.T) {
	tests := []struct {
		idxSupports MediaType
		req         MediaType
		want        bool
	}{
		{MediaTypeBoth, MediaTypeMovie, true},
		{MediaTypeBoth, MediaTypeTV, true},
		{MediaTypeBoth, MediaTypeBoth, true},
		{MediaTypeMovie, MediaTypeBoth, true},
		{MediaTypeTV, MediaTypeBoth, true},
		{MediaTypeMovie, MediaTypeMovie, true},
		{MediaTypeMovie, MediaTypeTV, false},
		{MediaTypeTV, MediaTypeTV, true},
		{MediaTypeTV, MediaTypeMovie, false},
	}

	for _, tt := range tests {
		idx := &mockIndexer{supports: tt.idxSupports}
		got := supportsMediaType(idx, tt.req)
		if got != tt.want {
			t.Errorf("supportsMediaType(%q, %q) = %v, want %v", tt.idxSupports, tt.req, got, tt.want)
		}
	}
}

// ── Registry.Search ───────────────────────────────────────────────────────────

func TestRegistry_Search_ReturnsSingleResult(t *testing.T) {
	r := &Registry{}
	r.Register(&mockIndexer{
		name: "idx", enabled: true, supports: MediaTypeBoth, priority: 1,
		results: []*SearchResult{
			{InfoHash: "abc", Title: "Movie 1080p", Quality: Quality1080p, Seeders: 10, Source: "idx"},
		},
	})

	results, errs := r.Search(&SearchRequest{MediaType: MediaTypeBoth, Quality: Quality1080p})
	if len(errs) != 0 {
		t.Fatalf("unexpected errors: %v", errs)
	}
	if len(results) != 1 {
		t.Fatalf("len(results) = %d, want 1", len(results))
	}
	if results[0].Title != "Movie 1080p" {
		t.Errorf("results[0].Title = %q, want %q", results[0].Title, "Movie 1080p")
	}
}

func TestRegistry_Search_DeduplicatesByInfoHashCaseInsensitive(t *testing.T) {
	r := &Registry{}
	// Two indexers return the same torrent with different casing in InfoHash.
	r.Register(&mockIndexer{
		name: "a", enabled: true, supports: MediaTypeBoth, priority: 1,
		results: []*SearchResult{
			{InfoHash: "ABC123", Seeders: 10, Quality: Quality1080p, Source: "a"},
		},
	})
	r.Register(&mockIndexer{
		name: "b", enabled: true, supports: MediaTypeBoth, priority: 2,
		results: []*SearchResult{
			{InfoHash: "abc123", Seeders: 50, Quality: Quality1080p, Source: "b"}, // more seeders
		},
	})

	results, errs := r.Search(&SearchRequest{MediaType: MediaTypeBoth, Quality: Quality1080p})
	if len(errs) != 0 {
		t.Fatalf("unexpected errors: %v", errs)
	}
	if len(results) != 1 {
		t.Fatalf("dedup failed: len(results) = %d, want 1", len(results))
	}
	// The copy with more seeders should survive.
	if results[0].Seeders != 50 {
		t.Errorf("expected 50 seeders (winner), got %d", results[0].Seeders)
	}
}

func TestRegistry_Search_DeduplicateLowerSeederLoses(t *testing.T) {
	r := &Registry{}
	r.Register(&mockIndexer{
		name: "a", enabled: true, supports: MediaTypeBoth, priority: 1,
		results: []*SearchResult{
			{InfoHash: "same", Seeders: 100, Quality: Quality1080p, Source: "a"},
			{InfoHash: "same", Seeders: 5, Quality: Quality1080p, Source: "a"}, // duplicate lower
		},
	})

	results, _ := r.Search(&SearchRequest{MediaType: MediaTypeBoth, Quality: Quality1080p})
	if len(results) != 1 {
		t.Fatalf("expected 1 result after dedup, got %d", len(results))
	}
	if results[0].Seeders != 100 {
		t.Errorf("expected high-seeder entry to win, got %d seeders", results[0].Seeders)
	}
}

func TestRegistry_Search_SkipsEmptyInfoHash(t *testing.T) {
	r := &Registry{}
	r.Register(&mockIndexer{
		name: "idx", enabled: true, supports: MediaTypeBoth, priority: 1,
		results: []*SearchResult{
			{InfoHash: "", Title: "no hash", Quality: Quality1080p, Seeders: 100},
			{InfoHash: "valid", Title: "has hash", Quality: Quality1080p, Seeders: 5},
		},
	})

	results, _ := r.Search(&SearchRequest{MediaType: MediaTypeBoth, Quality: Quality1080p})
	if len(results) != 1 {
		t.Fatalf("expected 1 result (empty hash skipped), got %d", len(results))
	}
	if results[0].InfoHash != "valid" {
		t.Errorf("expected 'valid' hash result, got %q", results[0].InfoHash)
	}
}

func TestRegistry_Search_CollectsErrors(t *testing.T) {
	r := &Registry{}
	r.Register(&mockIndexer{
		name: "failing", enabled: true, supports: MediaTypeBoth, priority: 1,
		err: errors.New("connection refused"),
	})

	results, errs := r.Search(&SearchRequest{MediaType: MediaTypeBoth})
	if len(errs) != 1 {
		t.Fatalf("len(errs) = %d, want 1", len(errs))
	}
	if len(results) != 0 {
		t.Fatalf("len(results) = %d, want 0", len(results))
	}
}

func TestRegistry_Search_MixedSuccessAndError(t *testing.T) {
	r := &Registry{}
	r.Register(&mockIndexer{
		name: "ok", enabled: true, supports: MediaTypeBoth, priority: 1,
		results: []*SearchResult{{InfoHash: "x", Quality: QualityAny, Seeders: 1, Source: "ok"}},
	})
	r.Register(&mockIndexer{
		name: "err", enabled: true, supports: MediaTypeBoth, priority: 2,
		err: errors.New("timeout"),
	})

	results, errs := r.Search(&SearchRequest{MediaType: MediaTypeBoth})
	if len(errs) != 1 {
		t.Errorf("expected 1 error, got %d", len(errs))
	}
	if len(results) != 1 {
		t.Errorf("expected 1 result from ok indexer, got %d", len(results))
	}
}

func TestRegistry_Search_SkipsUnsupportedMediaType(t *testing.T) {
	r := &Registry{}
	// Movie-only indexer.
	r.Register(&mockIndexer{
		name: "movies-only", enabled: true, supports: MediaTypeMovie, priority: 1,
		results: []*SearchResult{
			{InfoHash: "hash1", Quality: QualityAny, Seeders: 5, Source: "movies-only"},
		},
	})

	// Search for TV shows — movies-only indexer must be skipped.
	results, _ := r.Search(&SearchRequest{MediaType: MediaTypeTV})
	if len(results) != 0 {
		t.Errorf("expected 0 results, movie indexer should not handle TV, got %d", len(results))
	}
}

func TestRegistry_Search_SortsByQualityThenSeeders(t *testing.T) {
	r := &Registry{}
	r.Register(&mockIndexer{
		name: "idx", enabled: true, supports: MediaTypeBoth, priority: 1,
		results: []*SearchResult{
			{InfoHash: "low", Title: "480p", Quality: Quality480p, Seeders: 999, Source: "idx"},
			{InfoHash: "high", Title: "1080p-high", Quality: Quality1080p, Seeders: 100, Source: "idx"},
			{InfoHash: "mid", Title: "720p", Quality: Quality720p, Seeders: 200, Source: "idx"},
			{InfoHash: "exact", Title: "1080p-exact", Quality: Quality1080p, Seeders: 50, Source: "idx"},
		},
	})

	results, _ := r.Search(&SearchRequest{MediaType: MediaTypeBoth, Quality: Quality1080p})
	if len(results) != 4 {
		t.Fatalf("expected 4 results, got %d", len(results))
	}
	// Both 1080p entries must come first (score 100), ordered by seeders desc.
	if results[0].Quality != Quality1080p {
		t.Errorf("results[0] quality = %q, want 1080p (exact match should rank first)", results[0].Quality)
	}
	if results[1].Quality != Quality1080p {
		t.Errorf("results[1] quality = %q, want 1080p", results[1].Quality)
	}
	// Among 1080p entries, more seeders first.
	if results[0].Seeders < results[1].Seeders {
		t.Errorf("results[0].Seeders (%d) < results[1].Seeders (%d), expected descending", results[0].Seeders, results[1].Seeders)
	}
}

func TestRegistry_Search_SortsByPriorityWhenQualityAndSeedersEqual(t *testing.T) {
	r := &Registry{}
	r.Register(&mockIndexer{
		name: "low-priority", enabled: true, supports: MediaTypeBoth, priority: 5,
		results: []*SearchResult{
			{InfoHash: "a", Quality: Quality1080p, Seeders: 10, Source: "low-priority"},
		},
	})
	r.Register(&mockIndexer{
		name: "high-priority", enabled: true, supports: MediaTypeBoth, priority: 1,
		results: []*SearchResult{
			{InfoHash: "b", Quality: Quality1080p, Seeders: 10, Source: "high-priority"},
		},
	})

	results, _ := r.Search(&SearchRequest{MediaType: MediaTypeBoth, Quality: Quality1080p})
	if len(results) != 2 {
		t.Fatalf("expected 2 results, got %d", len(results))
	}
	// Lower priority number = higher precedence.
	if results[0].Source != "high-priority" {
		t.Errorf("results[0].Source = %q, want %q", results[0].Source, "high-priority")
	}
}

func TestRegistry_Search_EmptyRegistry(t *testing.T) {
	r := &Registry{}
	results, errs := r.Search(&SearchRequest{MediaType: MediaTypeBoth})
	if len(results) != 0 {
		t.Errorf("expected 0 results from empty registry, got %d", len(results))
	}
	if len(errs) != 0 {
		t.Errorf("expected 0 errors from empty registry, got %d", len(errs))
	}
}

// ── Registry.Health ───────────────────────────────────────────────────────────

func TestRegistry_Health_ReturnsAllIndexers(t *testing.T) {
	r := &Registry{}
	r.Register(&mockIndexer{name: "a", enabled: true, supports: MediaTypeBoth, priority: 1})
	r.Register(&mockIndexer{name: "b", enabled: false, supports: MediaTypeBoth, priority: 2})

	health := r.Health()
	if len(health) != 2 {
		t.Fatalf("Health() len = %d, want 2", len(health))
	}
}

func TestRegistry_Health_ReflectsEnabledState(t *testing.T) {
	r := &Registry{}
	r.Register(&mockIndexer{name: "enabled", enabled: true, supports: MediaTypeBoth, priority: 1})
	r.Register(&mockIndexer{name: "disabled", enabled: false, supports: MediaTypeBoth, priority: 2})

	health := r.Health()
	for _, h := range health {
		switch h.Name {
		case "enabled":
			if !h.Enabled {
				t.Errorf("Health entry for 'enabled' has Enabled=false")
			}
		case "disabled":
			if h.Enabled {
				t.Errorf("Health entry for 'disabled' has Enabled=true")
			}
		}
	}
}

// ── indexerStatTracker ────────────────────────────────────────────────────────

func TestIndexerStatTracker_Initial(t *testing.T) {
	tracker := &indexerStatTracker{}
	snap := tracker.snapshot("test", true)

	if snap.TotalSearches != 0 {
		t.Errorf("TotalSearches = %d, want 0", snap.TotalSearches)
	}
	if snap.TotalErrors != 0 {
		t.Errorf("TotalErrors = %d, want 0", snap.TotalErrors)
	}
	if snap.ErrorRate != 0 {
		t.Errorf("ErrorRate = %f, want 0", snap.ErrorRate)
	}
	if snap.AvgResponseMs != 0 {
		t.Errorf("AvgResponseMs = %f, want 0", snap.AvgResponseMs)
	}
}

func TestIndexerStatTracker_RecordSuccess(t *testing.T) {
	tracker := &indexerStatTracker{}
	tracker.recordSuccess(150.0, 7)
	snap := tracker.snapshot("test", true)

	if snap.TotalSearches != 1 {
		t.Errorf("TotalSearches = %d, want 1", snap.TotalSearches)
	}
	if snap.LastResultCount != 7 {
		t.Errorf("LastResultCount = %d, want 7", snap.LastResultCount)
	}
	if snap.AvgResponseMs != 150.0 {
		t.Errorf("AvgResponseMs = %f, want 150.0", snap.AvgResponseMs)
	}
	if snap.ErrorRate != 0 {
		t.Errorf("ErrorRate = %f, want 0", snap.ErrorRate)
	}
}

func TestIndexerStatTracker_RecordError(t *testing.T) {
	tracker := &indexerStatTracker{}
	tracker.recordError()
	snap := tracker.snapshot("test", true)

	if snap.TotalSearches != 1 {
		t.Errorf("TotalSearches = %d, want 1", snap.TotalSearches)
	}
	if snap.TotalErrors != 1 {
		t.Errorf("TotalErrors = %d, want 1", snap.TotalErrors)
	}
	if snap.ErrorRate != 1.0 {
		t.Errorf("ErrorRate = %f, want 1.0", snap.ErrorRate)
	}
}

func TestIndexerStatTracker_MixedErrorRate(t *testing.T) {
	tracker := &indexerStatTracker{}
	tracker.recordSuccess(100, 5)
	tracker.recordError()
	snap := tracker.snapshot("test", true)

	if snap.TotalSearches != 2 {
		t.Errorf("TotalSearches = %d, want 2", snap.TotalSearches)
	}
	if snap.TotalErrors != 1 {
		t.Errorf("TotalErrors = %d, want 1", snap.TotalErrors)
	}
	// ErrorRate = 1/2 = 0.5
	const wantRate = 0.5
	if snap.ErrorRate != wantRate {
		t.Errorf("ErrorRate = %f, want %f", snap.ErrorRate, wantRate)
	}
}

func TestIndexerStatTracker_RollingWindowAverage(t *testing.T) {
	tracker := &indexerStatTracker{}

	// Record 15 successes; window holds only the last 10.
	for i := 1; i <= 15; i++ {
		tracker.recordSuccess(float64(i*10), 1)
	}
	snap := tracker.snapshot("test", true)

	// TotalSearches should be 15.
	if snap.TotalSearches != 15 {
		t.Errorf("TotalSearches = %d, want 15", snap.TotalSearches)
	}
	// Average of last 10 values: 60+70+80+90+100+110+120+130+140+150 = 1050 / 10 = 105.
	const wantAvg = 105.0
	if snap.AvgResponseMs != wantAvg {
		t.Errorf("AvgResponseMs = %f, want %f", snap.AvgResponseMs, wantAvg)
	}
}

func TestIndexerStatTracker_SnapshotNameAndEnabled(t *testing.T) {
	tracker := &indexerStatTracker{}
	snap := tracker.snapshot("my-indexer", false)

	if snap.Name != "my-indexer" {
		t.Errorf("Name = %q, want %q", snap.Name, "my-indexer")
	}
	if snap.Enabled {
		t.Errorf("Enabled = true, want false")
	}
}

// ── NewDefaultRegistry ────────────────────────────────────────────────────────

func TestNewDefaultRegistry_RegistersBuiltInIndexers(t *testing.T) {
	// NewDefaultRegistry should register all 8 built-in indexers.
	// Their constructors do not make network calls — they only store config.
	r := NewDefaultRegistry()
	all := r.All()
	if len(all) == 0 {
		t.Fatal("NewDefaultRegistry registered no indexers")
	}
	// Verify every registered indexer has a non-empty Name.
	for _, idx := range all {
		if idx.Name() == "" {
			t.Errorf("indexer at position has empty Name()")
		}
	}
}

func TestNewDefaultRegistry_IndexersHaveConsistentEnabledState(t *testing.T) {
	// Verify we can read the enabled state of all registered indexers without panic.
	r := NewDefaultRegistry()
	for _, idx := range r.All() {
		// Just confirming the call doesn't panic; the default state is determined
		// by each concrete indexer and may be true or false.
		_ = idx.Enabled()
	}
}

func TestNewDefaultRegistry_SetEnabled(t *testing.T) {
	r := NewDefaultRegistry()
	all := r.All()
	if len(all) == 0 {
		t.Skip("no indexers registered")
	}

	// Enable the first indexer.
	all[0].SetEnabled(true)
	if !all[0].Enabled() {
		t.Errorf("SetEnabled(true) did not enable indexer %q", all[0].Name())
	}

	// Disable it again.
	all[0].SetEnabled(false)
	if all[0].Enabled() {
		t.Errorf("SetEnabled(false) did not disable indexer %q", all[0].Name())
	}
}

func TestNewDefaultRegistry_Health(t *testing.T) {
	r := NewDefaultRegistry()
	health := r.Health()
	// Health should have an entry for every registered indexer.
	if len(health) != len(r.All()) {
		t.Errorf("Health() returned %d entries, want %d", len(health), len(r.All()))
	}
}
