package indexers

import (
	"testing"

	"pgregory.net/rapid"
)

var allQualities = []Quality{Quality2160p, Quality1080p, Quality720p, Quality480p, QualityAny}

// Property: exact match always scores 100 for every Quality value.
func TestQualityScore_ExactMatchProp(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		q := rapid.SampledFrom(allQualities).Draw(t, "q")
		if score := QualityScore(q, q); score != 100 {
			t.Fatalf("QualityScore(%q, %q) = %d, want 100", q, q, score)
		}
	})
}

// Property: score is always in [0, 100] for any pair of qualities.
func TestQualityScore_BoundedProp(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		p := rapid.SampledFrom(allQualities).Draw(t, "preferred")
		a := rapid.SampledFrom(allQualities).Draw(t, "available")
		score := QualityScore(p, a)
		if score < 0 || score > 100 {
			t.Fatalf("QualityScore(%q, %q) = %d, want value in [0, 100]", p, a, score)
		}
	})
}

// Property: QualityScore(QualityAny, q) == 80 for every non-Any quality.
// QualityAny preferred means "any quality is equally acceptable" → fixed score.
func TestQualityScore_AnyPreferredAlways80Prop(t *testing.T) {
	nonAny := []Quality{Quality2160p, Quality1080p, Quality720p, Quality480p}
	rapid.Check(t, func(t *rapid.T) {
		q := rapid.SampledFrom(nonAny).Draw(t, "q")
		if score := QualityScore(QualityAny, q); score != 80 {
			t.Fatalf("QualityScore(QualityAny, %q) = %d, want 80", q, score)
		}
	})
}

// Property: score is symmetric when neither argument is QualityAny.
// QualityAny creates deliberate asymmetry by design, so it is excluded here.
func TestQualityScore_SymmetricForNonAnyProp(t *testing.T) {
	nonAny := []Quality{Quality2160p, Quality1080p, Quality720p, Quality480p}
	rapid.Check(t, func(t *rapid.T) {
		p := rapid.SampledFrom(nonAny).Draw(t, "preferred")
		a := rapid.SampledFrom(nonAny).Draw(t, "available")
		fwd := QualityScore(p, a)
		rev := QualityScore(a, p)
		if fwd != rev {
			t.Fatalf("QualityScore(%q, %q) = %d != QualityScore(%q, %q) = %d (not symmetric)",
				p, a, fwd, a, p, rev)
		}
	})
}

// Property: ParseQuality always returns one of the five defined Quality values.
func TestParseQuality_AlwaysInKnownSetProp(t *testing.T) {
	rapid.Check(t, func(t *rapid.T) {
		title := rapid.String().Draw(t, "title")
		q := ParseQuality(title)
		for _, known := range allQualities {
			if q == known {
				return
			}
		}
		t.Fatalf("ParseQuality(%q) = %q is not a known Quality value", title, q)
	})
}

// Property: a title that contains any 2160p marker always yields Quality2160p.
// The 2160p branch is checked first in ParseQuality, so it wins regardless of
// what else appears in the string.
func TestParseQuality_2160pMarkerAlwaysWinsProp(t *testing.T) {
	markers := []string{"2160p", "2160P", "4K", "4k", "UHD", "uhd", "Uhd"}
	rapid.Check(t, func(t *rapid.T) {
		prefix := rapid.String().Draw(t, "prefix")
		suffix := rapid.String().Draw(t, "suffix")
		marker := rapid.SampledFrom(markers).Draw(t, "marker")
		// Embed the marker so it is unambiguously present as a token.
		title := prefix + " " + marker + " " + suffix
		if q := ParseQuality(title); q != Quality2160p {
			t.Fatalf("ParseQuality(%q) = %q, want Quality2160p (marker = %q)", title, q, marker)
		}
	})
}
