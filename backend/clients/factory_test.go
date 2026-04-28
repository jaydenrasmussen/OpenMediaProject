package clients

import (
	"strings"
	"testing"
)

// TestNewTorrentClient_KnownTypes verifies that every documented client type
// produces a non-nil, non-error client without making any network calls.
// The constructors only store configuration; connections happen lazily.
func TestNewTorrentClient_KnownTypes(t *testing.T) {
	knownTypes := []TorrentClientType{
		ClientTypeRuTorrent,
		ClientTypeTransmission,
		ClientTypeDeluge,
		ClientTypeQBittorrent,
		ClientTypeSABnzbd,
	}

	for _, ct := range knownTypes {
		t.Run(string(ct), func(t *testing.T) {
			client, err := NewTorrentClient(TorrentClientConfig{
				ClientType: ct,
				BaseURL:    "http://localhost:9091",
				Username:   "testuser",
				Password:   "testpass",
			})
			if err != nil {
				t.Errorf("NewTorrentClient(%q) error = %v", ct, err)
			}
			if client == nil {
				t.Errorf("NewTorrentClient(%q) returned nil client", ct)
			}
		})
	}
}

func TestNewTorrentClient_UnknownType_ReturnsError(t *testing.T) {
	_, err := NewTorrentClient(TorrentClientConfig{
		ClientType: TorrentClientType("totally-unknown"),
		BaseURL:    "http://localhost",
	})
	if err == nil {
		t.Error("expected error for unknown client type, got nil")
	}
}

func TestNewTorrentClient_UnknownType_ErrorContainsTypeName(t *testing.T) {
	const unknown = "does-not-exist"
	_, err := NewTorrentClient(TorrentClientConfig{
		ClientType: TorrentClientType(unknown),
	})
	if err == nil {
		t.Fatal("expected error, got nil")
	}
	if !strings.Contains(err.Error(), unknown) {
		t.Errorf("error %q does not mention the unknown type %q", err.Error(), unknown)
	}
}

// TestClientTypeConstants validates the string values of each constant so that
// configuration files and environment variables match documented values.
func TestClientTypeConstants(t *testing.T) {
	tests := []struct {
		constant TorrentClientType
		want     string
	}{
		{ClientTypeRuTorrent, "rutorrent"},
		{ClientTypeTransmission, "transmission"},
		{ClientTypeDeluge, "deluge"},
		{ClientTypeQBittorrent, "qbittorrent"},
		{ClientTypeSABnzbd, "sabnzbd"},
	}
	for _, tt := range tests {
		if string(tt.constant) != tt.want {
			t.Errorf("constant value = %q, want %q", tt.constant, tt.want)
		}
	}
}
