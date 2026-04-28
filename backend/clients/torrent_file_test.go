package clients

import (
	"testing"
)

// ── IsTorrentFile ─────────────────────────────────────────────────────────────

func TestIsTorrentFile_ValidPrefixes(t *testing.T) {
	valid := [][]byte{
		[]byte("d8:announceSomeMoreData"),
		[]byte("d7:commentMoreData"),
		[]byte("d10:created byMoreData"),
	}
	for _, data := range valid {
		if !IsTorrentFile(data) {
			t.Errorf("IsTorrentFile(%q) = false, want true", string(data[:min(len(data), 20)]))
		}
	}
}

func TestIsTorrentFile_InvalidData(t *testing.T) {
	invalid := [][]byte{
		[]byte("not a torrent"),
		[]byte("PK\x03\x04"), // ZIP header
		[]byte(""),
		[]byte("d9:announce"), // wrong length prefix
	}
	for _, data := range invalid {
		if IsTorrentFile(data) {
			t.Errorf("IsTorrentFile(%q) = true, want false", string(data))
		}
	}
}

// ── IsMagnetLink ──────────────────────────────────────────────────────────────

func TestIsMagnetLink_ValidMagnet(t *testing.T) {
	urls := []string{
		"magnet:?xt=urn:btih:abc123&dn=Some+Movie",
		"magnet:?xt=urn:btih:deadbeef",
	}
	for _, u := range urls {
		if !IsMagnetLink(u) {
			t.Errorf("IsMagnetLink(%q) = false, want true", u)
		}
	}
}

func TestIsMagnetLink_NonMagnet(t *testing.T) {
	urls := []string{
		"http://example.com/file.torrent",
		"https://example.com/file.torrent",
		"",
		"ftp://tracker.example.com",
	}
	for _, u := range urls {
		if IsMagnetLink(u) {
			t.Errorf("IsMagnetLink(%q) = true, want false", u)
		}
	}
}

// ── ProcessTorrentFile ────────────────────────────────────────────────────────

func TestProcessTorrentFile_ReturnsMetadata(t *testing.T) {
	data := []byte("d8:announce12:example.com13:creation datei1000ee")
	tf, err := ProcessTorrentFile(data, "test.torrent")
	if err != nil {
		t.Fatalf("ProcessTorrentFile error = %v", err)
	}
	if tf == nil {
		t.Fatal("ProcessTorrentFile returned nil")
	}
	if tf.Hash == "" {
		t.Error("expected non-empty hash")
	}
	if tf.Name != "test.torrent" {
		t.Errorf("Name = %q, want %q", tf.Name, "test.torrent")
	}
	if tf.Base64Data == "" {
		t.Error("expected non-empty Base64Data")
	}
	if tf.Date <= 0 {
		t.Error("expected positive Date timestamp")
	}
}

func TestProcessTorrentFile_EmptyData(t *testing.T) {
	tf, err := ProcessTorrentFile([]byte{}, "empty.torrent")
	if err != nil {
		t.Fatalf("ProcessTorrentFile error = %v", err)
	}
	if tf == nil {
		t.Fatal("ProcessTorrentFile returned nil for empty data")
	}
	// Hash is still computed (empty sha1).
	if tf.Hash == "" {
		t.Error("expected hash even for empty data")
	}
}

// ── min helper ────────────────────────────────────────────────────────────────

func min(a, b int) int {
	if a < b {
		return a
	}
	return b
}
