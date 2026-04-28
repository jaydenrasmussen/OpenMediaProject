package models

import "time"

// ReleaseProfile defines named term lists for filtering search results.
// Enabled profiles are applied in the download pipeline after indexer search:
//   - Require: release title must contain ALL of these terms (case-insensitive)
//   - Reject:  release title must NOT contain ANY of these terms (case-insensitive)
//   - Prefer:  release title gets a score boost if it contains any of these terms
type ReleaseProfile struct {
	ID        string    `json:"id"`
	Name      string    `json:"name"`
	Require   []string  `json:"require"`
	Reject    []string  `json:"reject"`
	Prefer    []string  `json:"prefer"`
	Enabled   bool      `json:"enabled"`
	CreatedAt time.Time `json:"created_at"`
}
