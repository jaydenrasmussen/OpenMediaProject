package indexers

import "testing"

// TestParseQuality verifies that quality tokens are extracted correctly from
// torrent titles regardless of casing or surrounding text.
func TestParseQuality(t *testing.T) {
	tests := []struct {
		title string
		want  Quality
	}{
		// 2160p variants
		{"Movie.Title.2160p.BluRay.x265", Quality2160p},
		{"Movie Title 4K REMUX", Quality2160p},
		{"4k.movie.bluray", Quality2160p},
		{"UHD.Blu-Ray.Remux", Quality2160p},
		{"movie.uhd.bluray", Quality2160p},
		// Case-insensitive
		{"title.2160P.web", Quality2160p},
		{"title.4k.web", Quality2160p},

		// 1080p variants
		{"Movie.Title.1080p.BluRay", Quality1080p},
		{"Movie Title 1080P WEB-DL", Quality1080p},
		{"1080p.hevc.x265", Quality1080p},

		// 720p variants
		{"Movie.Title.720p.HDTV", Quality720p},
		{"show.s01e01.720P.web", Quality720p},

		// 480p variants
		{"Movie.Title.480p.WEB", Quality480p},
		{"old.movie.480P.dvdrip", Quality480p},

		// Unknown / none
		{"Movie.Title.HDTV.XviD", QualityAny},
		{"no quality info here", QualityAny},
		{"", QualityAny},

		// 2160p wins when multiple tokens present
		{"2160p.and.also.1080p", Quality2160p},
	}

	for _, tt := range tests {
		t.Run(tt.title, func(t *testing.T) {
			got := ParseQuality(tt.title)
			if got != tt.want {
				t.Errorf("ParseQuality(%q) = %q, want %q", tt.title, got, tt.want)
			}
		})
	}
}

// TestQualityScore_ExactMatch verifies that an exact preferred==available match
// always returns 100.
func TestQualityScore_ExactMatch(t *testing.T) {
	for _, q := range []Quality{Quality2160p, Quality1080p, Quality720p, Quality480p, QualityAny} {
		if score := QualityScore(q, q); score != 100 {
			t.Errorf("QualityScore(%q, %q) = %d, want 100", q, q, score)
		}
	}
}

// TestQualityScore_PreferredAny verifies that when the caller doesn't care
// about quality every available quality receives score 80.
func TestQualityScore_PreferredAny(t *testing.T) {
	for _, q := range []Quality{Quality2160p, Quality1080p, Quality720p, Quality480p} {
		if score := QualityScore(QualityAny, q); score != 80 {
			t.Errorf("QualityScore(QualityAny, %q) = %d, want 80", q, score)
		}
	}
}

// TestQualityScore_DistanceBased exercises the step-down scoring logic.
//
// QualityFallbackOrder = [1080p(0), 720p(1), 480p(2), any(3)]
// Quality2160p is not in the list → qualityIndex returns -1.
func TestQualityScore_DistanceBased(t *testing.T) {
	tests := []struct {
		preferred Quality
		available Quality
		want      int
	}{
		// distance 1 → 80 - 20 = 60
		{Quality1080p, Quality720p, 60},
		{Quality720p, Quality1080p, 60},

		// distance 2 → 80 - 40 = 40
		{Quality1080p, Quality480p, 40},
		{Quality480p, Quality1080p, 40},

		// distance 3 → 80 - 60 = 20
		{Quality1080p, QualityAny, 20},

		// 2160p (index -1) vs 1080p (index 0): distance 1 → 60
		{Quality2160p, Quality1080p, 60},
		// 2160p (index -1) vs 720p (index 1): distance 2 → 40
		{Quality2160p, Quality720p, 40},
		// 2160p (index -1) vs 480p (index 2): distance 3 → 20
		{Quality2160p, Quality480p, 20},
		// 2160p (index -1) vs any (index 3): distance 4 → 80-80 = 0 (floor)
		{Quality2160p, QualityAny, 0},
	}

	for _, tt := range tests {
		got := QualityScore(tt.preferred, tt.available)
		if got != tt.want {
			t.Errorf("QualityScore(%q, %q) = %d, want %d", tt.preferred, tt.available, got, tt.want)
		}
	}
}

// TestQualityScore_NeverNegative verifies the floor-at-zero guard.
func TestQualityScore_NeverNegative(t *testing.T) {
	// The maximum possible distance with current constants is 4 (2160p vs any),
	// which produces exactly 0.  Confirm the return value is non-negative.
	score := QualityScore(Quality2160p, QualityAny)
	if score < 0 {
		t.Errorf("QualityScore should never be negative, got %d", score)
	}
}

// TestQualityIndex_NotInOrder verifies the sentinel -1 path of qualityIndex
// indirectly via QualityScore (2160p is intentionally absent from the
// fallback order).
func TestQualityIndex_NotInOrder(t *testing.T) {
	// 2160p vs itself → exact match path returns 100, not the distance path.
	if s := QualityScore(Quality2160p, Quality2160p); s != 100 {
		t.Errorf("expected 100 for exact match, got %d", s)
	}
}
