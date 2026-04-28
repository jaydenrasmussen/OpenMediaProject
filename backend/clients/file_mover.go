package clients

import (
	"fmt"
	"io"
	"os"
	"path/filepath"
	"regexp"
	"strings"

	"backend/clients/indexers"
	"backend/models"

	"github.com/rs/zerolog/log"
)

// videoExtensions is the set of file extensions treated as primary video files.
var videoExtensions = map[string]bool{
	".mkv": true, ".mp4": true, ".avi": true, ".mov": true,
	".wmv": true, ".m4v": true, ".ts": true, ".m2ts": true, ".webm": true,
}

// subtitleNFOExtensions are moved alongside the video into the same directory.
var subtitleNFOExtensions = map[string]bool{
	".srt": true, ".ass": true, ".sub": true, ".vtt": true, ".nfo": true,
}

// episodePattern matches S<NN>E<NN> in a filename (case-insensitive).
var episodePattern = regexp.MustCompile(`(?i)S(\d{2})E(\d{2})`)

// FileMover represents the file mover client.
type FileMover struct {
	movieDir  string
	tvShowDir string
}

// NewFileMover creates a new FileMover client.
func NewFileMover(movieDir, tvShowDir string) *FileMover {
	return &FileMover{
		movieDir:  movieDir,
		tvShowDir: tvShowDir,
	}
}

// sanitise replaces characters that are illegal in filenames with a hyphen and
// trims surrounding whitespace so the result is safe on all major filesystems.
func sanitise(s string) string {
	illegal := []string{"/", `\`, ":", "*", "?", `"`, "<", ">", "|"}
	for _, ch := range illegal {
		s = strings.ReplaceAll(s, ch, "-")
	}
	return strings.TrimSpace(s)
}

// movieYear extracts the 4-digit year from movie.ReleaseDate ("YYYY-MM-DD").
// Returns an empty string when the date is absent or too short.
func movieYear(movie *models.Movie) string {
	if len(movie.ReleaseDate) >= 4 {
		return movie.ReleaseDate[:4]
	}
	return ""
}

// movieFolderName returns "<Title> (<Year>)" or just "<Title>" when year is unknown.
func movieFolderName(movie *models.Movie) string {
	title := sanitise(movie.Title)
	year := movieYear(movie)
	if year == "" {
		return title
	}
	return fmt.Sprintf("%s (%s)", title, year)
}

// tvSeasonDir returns the full path to the season directory:
//
//	<tvShowDir>/<ShowName>/Season <NN>
func (m *FileMover) tvSeasonDir(show *models.TVShow, ep *models.Episode) string {
	return filepath.Join(m.tvShowDir, sanitise(show.Name), fmt.Sprintf("Season %02d", ep.Season))
}

// tvBaseName returns the base filename (no extension) for a TV episode:
//
//	<ShowName> - S<NN>E<NN> - <EpisodeTitle>
func tvBaseName(show *models.TVShow, ep *models.Episode) string {
	return fmt.Sprintf("%s - S%02dE%02d - %s",
		sanitise(show.Name), ep.Season, ep.Number, sanitise(ep.Title))
}

// moveOrCopy moves src to dst using os.Rename; if that fails (e.g. cross-device)
// it falls back to a copy-then-delete.
func moveOrCopy(src, dst string) error {
	if err := os.Rename(src, dst); err == nil {
		return nil
	}
	if err := copyFile(src, dst); err != nil {
		return fmt.Errorf("copy fallback failed: %w", err)
	}
	if err := os.Remove(src); err != nil {
		log.Warn().Err(err).Str("src", src).Msg("Could not remove source file after copy-fallback")
	}
	return nil
}

// copyFile copies the file at src to dst, creating or truncating dst.
func copyFile(src, dst string) error {
	in, err := os.Open(src)
	if err != nil {
		return err
	}
	defer in.Close()

	out, err := os.Create(dst)
	if err != nil {
		return err
	}
	if _, err = io.Copy(out, in); err != nil {
		out.Close()
		return err
	}
	return out.Close()
}

// MoveMovieFile moves a video file into the structured movie library:
//
//	<movieDir>/<Title> (<Year>)/<Title> (<Year>)<ext>
//
// Non-video files (subtitles, NFO) are moved into the same directory.
// Other files are placed in an Extras/ sub-directory.
// Returns the final destination path of the primary file.
func (m *FileMover) MoveMovieFile(sourcePath string, movie *models.Movie) (string, error) {
	ext := strings.ToLower(filepath.Ext(sourcePath))
	folderName := movieFolderName(movie)
	destDir := filepath.Join(m.movieDir, folderName)

	var finalDest string

	if videoExtensions[ext] {
		finalDest = filepath.Join(destDir, folderName+ext)
	} else if subtitleNFOExtensions[ext] {
		langCode := extractLangCode(filepath.Base(sourcePath), ext)
		if langCode != "" {
			finalDest = filepath.Join(destDir, folderName+"."+langCode+ext)
		} else {
			finalDest = filepath.Join(destDir, folderName+ext)
		}
	} else {
		// Extras
		extrasDir := filepath.Join(destDir, "Extras")
		if err := os.MkdirAll(extrasDir, 0755); err != nil {
			return "", fmt.Errorf("mkdir extras: %w", err)
		}
		finalDest = filepath.Join(extrasDir, filepath.Base(sourcePath))
	}

	if err := os.MkdirAll(filepath.Dir(finalDest), 0755); err != nil {
		return "", fmt.Errorf("mkdir: %w", err)
	}

	log.Info().Str("src", sourcePath).Str("dst", finalDest).Msg("Moving movie file")
	if err := moveOrCopy(sourcePath, finalDest); err != nil {
		return "", fmt.Errorf("move movie file: %w", err)
	}
	log.Info().Str("dst", finalDest).Msg("Movie file moved successfully")
	return finalDest, nil
}

// MoveTVFile moves a video file into the structured TV library:
//
//	<tvShowDir>/<ShowName>/Season <NN>/<ShowName> - S<NN>E<NN> - <EpisodeTitle><ext>
//
// Non-video files (subtitles, NFO) are moved into the same directory.
// Other files are placed in an Extras/ sub-directory inside the season directory.
// Returns the final destination path of the primary file.
func (m *FileMover) MoveTVFile(sourcePath string, show *models.TVShow, ep *models.Episode) (string, error) {
	ext := strings.ToLower(filepath.Ext(sourcePath))
	seasonDir := m.tvSeasonDir(show, ep)
	base := tvBaseName(show, ep)

	var finalDest string

	if videoExtensions[ext] {
		finalDest = filepath.Join(seasonDir, base+ext)
	} else if subtitleNFOExtensions[ext] {
		// Use the episode-formatted base name, preserving any language code.
		// e.g. "Show.S01E01.720p.en.srt" → "Show Name - S01E01 - Title.en.srt"
		langCode := extractLangCode(filepath.Base(sourcePath), ext)
		if langCode != "" {
			finalDest = filepath.Join(seasonDir, base+"."+langCode+ext)
		} else {
			finalDest = filepath.Join(seasonDir, base+ext)
		}
	} else {
		extrasDir := filepath.Join(seasonDir, "Extras")
		if err := os.MkdirAll(extrasDir, 0755); err != nil {
			return "", fmt.Errorf("mkdir extras: %w", err)
		}
		finalDest = filepath.Join(extrasDir, filepath.Base(sourcePath))
	}

	if err := os.MkdirAll(filepath.Dir(finalDest), 0755); err != nil {
		return "", fmt.Errorf("mkdir: %w", err)
	}

	log.Info().Str("src", sourcePath).Str("dst", finalDest).Msg("Moving TV file")
	if err := moveOrCopy(sourcePath, finalDest); err != nil {
		return "", fmt.Errorf("move TV file: %w", err)
	}
	log.Info().Str("dst", finalDest).Msg("TV file moved successfully")
	return finalDest, nil
}

// MoveFile is the legacy compatibility shim — moves the file to the flat movieDir
// or tvShowDir root without any structured sub-directory layout.
// Kept for backward compatibility with CheckForCompletedTorrents.
func (m *FileMover) MoveFile(sourcePath, mediaType string) error {
	var destDir string
	switch mediaType {
	case "movie":
		destDir = m.movieDir
	case "tvshow":
		destDir = m.tvShowDir
	default:
		log.Warn().Str("mediaType", mediaType).Msg("Unknown media type")
	}

	if destDir == "" {
		log.Info().Str("source", sourcePath).Msg("No destination directory configured for media type, skipping move")
		return nil
	}

	destPath := filepath.Join(destDir, filepath.Base(sourcePath))
	log.Info().Str("source", sourcePath).Str("destination", destPath).Msg("Moving file (legacy)")
	if err := moveOrCopy(sourcePath, destPath); err != nil {
		log.Error().Err(err).Msg("Failed to move file")
		return err
	}
	log.Info().Str("destination", destPath).Msg("File moved successfully")
	return nil
}

// IsMovieDuplicate reports whether a file of equal or better quality already
// exists in the movie's target directory.
// It walks <movieDir>/<Title> (<Year>)/ and compares quality parsed from filenames.
func (m *FileMover) IsMovieDuplicate(movie *models.Movie, incomingQuality string) bool {
	targetDir := filepath.Join(m.movieDir, movieFolderName(movie))
	incoming := indexers.ParseQuality(incomingQuality)

	found := false
	_ = filepath.Walk(targetDir, func(path string, info os.FileInfo, err error) error {
		if err != nil || info.IsDir() {
			return nil
		}
		ext := strings.ToLower(filepath.Ext(path))
		if !videoExtensions[ext] {
			return nil
		}
		existing := indexers.ParseQuality(filepath.Base(path))
		// Existing is "as good or better" when its score vs the incoming quality
		// is >= the incoming score vs itself (i.e. existing quality is >= incoming).
		if qualityGTE(existing, incoming) {
			found = true
			return fmt.Errorf("stop") // short-circuit
		}
		return nil
	})
	return found
}

// IsTVDuplicate reports whether a file of equal or better quality already exists
// for the given show/episode in the target season directory.
func (m *FileMover) IsTVDuplicate(show *models.TVShow, ep *models.Episode, incomingQuality string) bool {
	seasonDir := m.tvSeasonDir(show, ep)
	incoming := indexers.ParseQuality(incomingQuality)
	epTag := fmt.Sprintf("S%02dE%02d", ep.Season, ep.Number)

	found := false
	_ = filepath.Walk(seasonDir, func(path string, info os.FileInfo, err error) error {
		if err != nil || info.IsDir() {
			return nil
		}
		ext := strings.ToLower(filepath.Ext(path))
		if !videoExtensions[ext] {
			return nil
		}
		name := filepath.Base(path)
		if !episodePattern.MatchString(name) {
			return nil
		}
		// Check the S/E tag matches this episode.
		m2 := episodePattern.FindString(strings.ToUpper(name))
		if !strings.EqualFold(m2, epTag) {
			return nil
		}
		existing := indexers.ParseQuality(name)
		if qualityGTE(existing, incoming) {
			found = true
			return fmt.Errorf("stop")
		}
		return nil
	})
	return found
}

// qualityGTE returns true when existing quality is greater than or equal to
// incoming quality using QualityFallbackOrder.
// Higher index in QualityFallbackOrder = lower quality.
// Quality2160p is treated as the highest tier (not in the fallback list).
func qualityGTE(existing, incoming indexers.Quality) bool {
	rankOf := func(q indexers.Quality) int {
		// 2160p is best: rank -1 (lower rank = better).
		if q == indexers.Quality2160p {
			return -1
		}
		for i, fq := range indexers.QualityFallbackOrder {
			if fq == q {
				return i
			}
		}
		// QualityAny or unknown filename — treat as equivalent to lowest known quality
		// so that an existing file (even with no quality in the name) prevents re-download.
		return len(indexers.QualityFallbackOrder) - 1
	}
	// existing >= incoming means existing rank <= incoming rank.
	return rankOf(existing) <= rankOf(incoming)
}

// extractLangCode attempts to extract a 2-3 letter language code from a subtitle
// filename. For example, "Show.S01E01.en.srt" → "en", "show.fr.ass" → "fr".
// Returns empty string if no language code is found.
func extractLangCode(filename, ext string) string {
	// Strip the extension, then check if the remaining name ends with .<2-3 letters>
	withoutExt := strings.TrimSuffix(filename, ext)
	// Also strip a secondary extension if present (e.g. ".en" from ".en.srt")
	inner := filepath.Ext(withoutExt)
	if inner == "" {
		return ""
	}
	// inner is like ".en" or ".fra"
	code := strings.TrimPrefix(inner, ".")
	if len(code) == 2 || len(code) == 3 {
		// Verify it's alphabetic
		for _, c := range code {
			if c < 'a' || c > 'z' {
				return ""
			}
		}
		return code
	}
	return ""
}
