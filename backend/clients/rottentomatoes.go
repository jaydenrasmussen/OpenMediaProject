package clients

// RTRating holds critics and audience scores from Rotten Tomatoes.
//
// This uses the public Algolia index that RT itself uses for its website
// search, which means no API key is required. The approach is a port of the
// one used by Seerr (https://github.com/seerr-team/seerr).
//
// Algolia credentials embedded in the RT website (publicly visible):
//   app-id  79FRDP12PN
//   api-key 175588f6e5f8319b27702e4cc4013561

import (
	"bytes"
	"encoding/json"
	"fmt"
	"math"
	"net/http"
	"net/url"
	"regexp"
	"strings"
)

const (
	rtAlgoliaURL  = "https://79frdp12pn-dsn.algolia.net/1/indexes/*/queries"
	rtAlgoliaKey  = "175588f6e5f8319b27702e4cc4013561"
	rtAlgoliaApp  = "79FRDP12PN"

	// Scoring tunables (mirrored from Seerr).
	rtInexactFactor = 0.25
	rtAltFactor     = 0.80
	rtYearPenalty   = 0.40
	rtMinScore      = 0.175
)

// RTRating holds the rating data from Rotten Tomatoes.
type RTRating struct {
	Title          string
	Year           int
	CriticsScore   int
	AudienceScore  int
	CertifiedFresh bool
	URL            string
}

// --- internal Algolia types ------------------------------------------------

type rtRequest struct {
	Requests []rtQuery `json:"requests"`
}

type rtQuery struct {
	IndexName string `json:"indexName"`
	Query     string `json:"query"`
	Params    string `json:"params"`
}

type rtResponse struct {
	Results []struct {
		Hits []rtHit `json:"hits"`
	} `json:"results"`
}

type rtHit struct {
	Title       string   `json:"title"`
	Titles      []string `json:"titles"`
	AKA         []string `json:"aka"`
	ReleaseYear int      `json:"releaseYear"`
	Vanity      string   `json:"vanity"`
	RottenTomatoes *struct {
		CriticsScore   int  `json:"criticsScore"`
		AudienceScore  int  `json:"audienceScore"`
		CertifiedFresh bool `json:"certifiedFresh"`
	} `json:"rottenTomatoes"`
}

// --- string normalisation --------------------------------------------------

var rtNonAlphaNum = regexp.MustCompile(`[^\p{L}\p{N} ]`)
var rtThePrefix = regexp.MustCompile(`(?i)\bthe\b ?`)

func rtNorm(s string) string {
	return strings.TrimSpace(rtNonAlphaNum.ReplaceAllString(strings.ToLower(s), ""))
}

// --- Jaro similarity (standard algorithm) ----------------------------------

func jaroSim(s1, s2 string) float64 {
	r1, r2 := []rune(s1), []rune(s2)
	l1, l2 := len(r1), len(r2)
	if l1 == 0 && l2 == 0 {
		return 1.0
	}
	if l1 == 0 || l2 == 0 {
		return 0.0
	}
	if s1 == s2 {
		return 1.0
	}
	matchDist := max(l1, l2)/2 - 1
	if matchDist < 0 {
		matchDist = 0
	}
	m1 := make([]bool, l1)
	m2 := make([]bool, l2)
	matches := 0
	for i := 0; i < l1; i++ {
		lo := max(0, i-matchDist)
		hi := min(l2, i+matchDist+1)
		for j := lo; j < hi; j++ {
			if !m2[j] && r1[i] == r2[j] {
				m1[i] = true
				m2[j] = true
				matches++
				break
			}
		}
	}
	if matches == 0 {
		return 0.0
	}
	trans := 0
	k := 0
	for i := 0; i < l1; i++ {
		if m1[i] {
			for !m2[k] {
				k++
			}
			if r1[i] != r2[k] {
				trans++
			}
			k++
		}
	}
	fm := float64(matches)
	return (fm/float64(l1) + fm/float64(l2) + (fm-float64(trans)/2)/fm) / 3
}

// titleSim returns 1 for exact match, quarter-jaro otherwise.
func titleSim(a, b string) float64 {
	if a == b {
		return 1.0
	}
	return jaroSim(a, b) * rtInexactFactor
}

// --- hit scoring -----------------------------------------------------------

func hitTitleScore(h rtHit, search string) float64 {
	all := append([]string{h.Title}, append(h.AKA, h.Titles...)...)
	best := 0.0
	for i, t := range all {
		factor := 1.0
		if i > 0 {
			factor = rtAltFactor
		}
		s := titleSim(rtNorm(t), search) * factor
		if s > best {
			best = s
		}
	}
	return best
}

func hitYearScore(h rtHit, year int) float64 {
	if year == 0 {
		return 1.0
	}
	diff := math.Abs(float64(h.ReleaseYear - year))
	s := 1.0 - diff*rtYearPenalty
	if s < 0 {
		return 0.0
	}
	return s
}

func hitScore(h rtHit, name string, year int) float64 {
	extra := 0.5
	if h.RottenTomatoes != nil {
		extra = 1.0
	}
	return hitTitleScore(h, name) * hitYearScore(h, year) * extra
}

func bestHit(hits []rtHit, name string, year int) *rtHit {
	var best *rtHit
	bestScore := rtMinScore
	for i := range hits {
		s := hitScore(hits[i], name, year)
		if s > bestScore {
			bestScore = s
			h := hits[i]
			best = &h
		}
	}
	return best
}

// --- Algolia search --------------------------------------------------------

func rtSearch(mediaType, query string) ([]rtHit, error) {
	filters := url.QueryEscape(fmt.Sprintf(`isEmsSearchable=1 AND type:"%s"`, mediaType))
	body := rtRequest{
		Requests: []rtQuery{{
			IndexName: "content_rt",
			Query:     query,
			Params:    "filters=" + filters + "&hitsPerPage=20",
		}},
	}
	payload, _ := json.Marshal(body)
	req, err := http.NewRequest("POST", rtAlgoliaURL, bytes.NewReader(payload))
	if err != nil {
		return nil, err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("x-algolia-api-key", rtAlgoliaKey)
	req.Header.Set("x-algolia-application-id", rtAlgoliaApp)

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	var result rtResponse
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, err
	}
	if len(result.Results) == 0 {
		return nil, nil
	}
	return result.Results[0].Hits, nil
}

// GetTVRatings searches the RT Algolia index for a TV show by title and year.
// Returns nil (no error) when no confident match is found.
func GetTVRatings(name string, year int) (*RTRating, error) {
	hits, err := rtSearch("tv", name)
	if err != nil {
		return nil, fmt.Errorf("rt tv search: %w", err)
	}
	norm := rtNorm(name)
	hit := bestHit(hits, norm, year)
	if hit == nil || hit.RottenTomatoes == nil {
		return nil, nil
	}
	return &RTRating{
		Title:          hit.Title,
		Year:           hit.ReleaseYear,
		CriticsScore:   hit.RottenTomatoes.CriticsScore,
		AudienceScore:  hit.RottenTomatoes.AudienceScore,
		CertifiedFresh: hit.RottenTomatoes.CertifiedFresh,
		URL:            "https://www.rottentomatoes.com/tv/" + hit.Vanity,
	}, nil
}

// GetMovieRatings searches the RT Algolia index for a movie by title and year.
// Returns nil (no error) when no confident match is found.
func GetMovieRatings(name string, year int) (*RTRating, error) {
	// Strip "the" from query (mirrors Seerr behaviour).
	query := strings.TrimSpace(rtThePrefix.ReplaceAllString(name, ""))
	hits, err := rtSearch("movie", query)
	if err != nil {
		return nil, fmt.Errorf("rt movie search: %w", err)
	}
	norm := rtNorm(name)
	hit := bestHit(hits, norm, year)
	if hit == nil || hit.RottenTomatoes == nil {
		return nil, nil
	}
	return &RTRating{
		Title:          hit.Title,
		Year:           hit.ReleaseYear,
		CriticsScore:   hit.RottenTomatoes.CriticsScore,
		AudienceScore:  hit.RottenTomatoes.AudienceScore,
		CertifiedFresh: hit.RottenTomatoes.CertifiedFresh,
		URL:            "https://www.rottentomatoes.com/m/" + hit.Vanity,
	}, nil
}
