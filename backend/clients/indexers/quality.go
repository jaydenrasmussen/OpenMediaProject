package indexers

import "strings"

// ParseQuality extracts a Quality from a torrent title string.
// Checks (case-insensitive) for "2160p"/"4K"/"UHD", "1080p", "720p", "480p".
// Returns QualityAny if none found.
func ParseQuality(title string) Quality {
	upper := strings.ToUpper(title)
	switch {
	case strings.Contains(upper, "2160P") ||
		strings.Contains(upper, "4K") ||
		strings.Contains(upper, "UHD"):
		return Quality2160p
	case strings.Contains(upper, "1080P"):
		return Quality1080p
	case strings.Contains(upper, "720P"):
		return Quality720p
	case strings.Contains(upper, "480P"):
		return Quality480p
	default:
		return QualityAny
	}
}

// qualityIndex returns the position of q in QualityFallbackOrder, or the last
// index + 1 if not found (which only happens for Quality2160p, since it is not
// in the fallback order by design — it is treated as an explicit preference).
func qualityIndex(q Quality) int {
	for i, fq := range QualityFallbackOrder {
		if fq == q {
			return i
		}
	}
	// Quality2160p is not in QualityFallbackOrder; treat it as index -1 so
	// that an exact-match on 2160p still scores 100 via the exact-match check.
	return -1
}

// QualityScore returns a numeric score for sorting (higher = better match to preferred).
// preferred is the desired quality. available is what we have.
// Exact match = 100. Score decreases based on distance in QualityFallbackOrder.
func QualityScore(preferred, available Quality) int {
	if preferred == available {
		return 100
	}

	// If preferred is QualityAny, every quality is equally good.
	if preferred == QualityAny {
		return 80
	}

	pi := qualityIndex(preferred)
	ai := qualityIndex(available)

	// Distance-based scoring: each step away subtracts 20, floor at 0.
	distance := ai - pi
	if distance < 0 {
		distance = -distance
	}
	score := 80 - (distance * 20)
	if score < 0 {
		score = 0
	}
	return score
}
