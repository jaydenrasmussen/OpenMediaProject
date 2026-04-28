package indexers

import (
	"sort"
	"strings"
	"sync"
	"time"
)

// IndexerStats is a public snapshot of runtime statistics for a single indexer.
type IndexerStats struct {
	Name            string    `json:"name"`
	Enabled         bool      `json:"enabled"`
	LastSuccess     time.Time `json:"last_success"`
	LastResultCount int       `json:"last_result_count"`
	TotalSearches   int       `json:"total_searches"`
	TotalErrors     int       `json:"total_errors"`
	ErrorRate       float64   `json:"error_rate"`
	AvgResponseMs   float64   `json:"avg_response_ms"`
}

// indexerStatTracker accumulates per-indexer runtime stats.
type indexerStatTracker struct {
	mu          sync.Mutex
	lastSuccess time.Time
	lastCount   int
	totalCalls  int
	totalErrors int
	// Rolling 10-window of response times in milliseconds.
	recentMs  [10]float64
	recentIdx int
	recentLen int
}

func (t *indexerStatTracker) recordSuccess(ms float64, count int) {
	t.mu.Lock()
	defer t.mu.Unlock()
	t.lastSuccess = time.Now()
	t.lastCount = count
	t.totalCalls++
	t.recentMs[t.recentIdx] = ms
	t.recentIdx = (t.recentIdx + 1) % 10
	if t.recentLen < 10 {
		t.recentLen++
	}
}

func (t *indexerStatTracker) recordError() {
	t.mu.Lock()
	defer t.mu.Unlock()
	t.totalCalls++
	t.totalErrors++
}

func (t *indexerStatTracker) snapshot(name string, enabled bool) IndexerStats {
	t.mu.Lock()
	defer t.mu.Unlock()

	var avgMs float64
	if t.recentLen > 0 {
		var sum float64
		for i := 0; i < t.recentLen; i++ {
			sum += t.recentMs[i]
		}
		avgMs = sum / float64(t.recentLen)
	}

	var errRate float64
	if t.totalCalls > 0 {
		errRate = float64(t.totalErrors) / float64(t.totalCalls)
	}

	return IndexerStats{
		Name:            name,
		Enabled:         enabled,
		LastSuccess:     t.lastSuccess,
		LastResultCount: t.lastCount,
		TotalSearches:   t.totalCalls,
		TotalErrors:     t.totalErrors,
		ErrorRate:       errRate,
		AvgResponseMs:   avgMs,
	}
}

// Registry holds all registered indexers and provides fan-out search.
type Registry struct {
	mu       sync.RWMutex
	indexers []Indexer
	statsMu  sync.RWMutex
	stats    map[string]*indexerStatTracker
}

// Register adds an indexer to the registry.
func (r *Registry) Register(idx Indexer) {
	r.mu.Lock()
	r.indexers = append(r.indexers, idx)
	r.mu.Unlock()

	r.statsMu.Lock()
	if r.stats == nil {
		r.stats = make(map[string]*indexerStatTracker)
	}
	r.stats[idx.Name()] = &indexerStatTracker{}
	r.statsMu.Unlock()
}

// All returns every registered indexer regardless of enabled state.
func (r *Registry) All() []Indexer {
	r.mu.RLock()
	defer r.mu.RUnlock()
	out := make([]Indexer, len(r.indexers))
	copy(out, r.indexers)
	return out
}

// Enabled returns only the indexers that are currently enabled.
func (r *Registry) Enabled() []Indexer {
	r.mu.RLock()
	defer r.mu.RUnlock()
	var out []Indexer
	for _, idx := range r.indexers {
		if idx.Enabled() {
			out = append(out, idx)
		}
	}
	return out
}

// supportsMediaType reports whether the indexer's supported type is compatible
// with the requested media type.
func supportsMediaType(idx Indexer, req MediaType) bool {
	s := idx.Supports()
	if s == MediaTypeBoth {
		return true
	}
	if req == MediaTypeBoth {
		return true
	}
	return s == req
}

// Search fans out concurrently to all enabled, compatible indexers. It
// collects results, deduplicates by InfoHash (keeping the copy with more
// seeders), then sorts by:
//  1. Quality match to req.Quality (higher QualityScore first)
//  2. Seeders descending
//  3. Source indexer Priority ascending
func (r *Registry) Search(req *SearchRequest) ([]*SearchResult, []error) {
	candidates := r.Enabled()

	type workerResult struct {
		results  []*SearchResult
		err      error
		idx      Indexer
		elapsedMs float64
	}

	ch := make(chan workerResult, len(candidates))
	var wg sync.WaitGroup

	for _, idx := range candidates {
		if !supportsMediaType(idx, req.MediaType) {
			continue
		}
		wg.Add(1)
		go func(ix Indexer) {
			defer wg.Done()
			start := time.Now()
			res, err := ix.Search(req)
			elapsed := float64(time.Since(start).Milliseconds())
			ch <- workerResult{results: res, err: err, idx: ix, elapsedMs: elapsed}
		}(idx)
	}

	// Close channel once all goroutines are done.
	go func() {
		wg.Wait()
		close(ch)
	}()

	// priorityOf maps indexer name → priority for sorting later.
	priorityOf := make(map[string]int, len(candidates))
	for _, idx := range candidates {
		priorityOf[idx.Name()] = idx.Priority()
	}

	// Collect results and errors.
	seen := make(map[string]*SearchResult) // keyed by lower-case InfoHash
	var errs []error

	for wr := range ch {
		// Record stats.
		r.statsMu.RLock()
		tracker := r.stats[wr.idx.Name()]
		r.statsMu.RUnlock()
		if tracker != nil {
			if wr.err != nil {
				tracker.recordError()
			} else {
				tracker.recordSuccess(wr.elapsedMs, len(wr.results))
			}
		}

		if wr.err != nil {
			errs = append(errs, wr.err)
			continue
		}
		for _, res := range wr.results {
			if res.InfoHash == "" {
				continue
			}
			key := strings.ToLower(res.InfoHash)
			if existing, ok := seen[key]; ok {
				if res.Seeders > existing.Seeders {
					seen[key] = res
				}
			} else {
				seen[key] = res
			}
		}
	}

	// Flatten deduplicated map into a slice.
	results := make([]*SearchResult, 0, len(seen))
	for _, res := range seen {
		results = append(results, res)
	}

	// Sort: quality score desc, seeders desc, priority asc.
	sort.Slice(results, func(i, j int) bool {
		ri, rj := results[i], results[j]

		si := QualityScore(req.Quality, ri.Quality)
		sj := QualityScore(req.Quality, rj.Quality)
		if si != sj {
			return si > sj
		}

		if ri.Seeders != rj.Seeders {
			return ri.Seeders > rj.Seeders
		}

		pi := priorityOf[ri.Source]
		pj := priorityOf[rj.Source]
		return pi < pj
	})

	return results, errs
}

// Health returns a stats snapshot for every registered indexer.
func (r *Registry) Health() []IndexerStats {
	r.mu.RLock()
	idxCopy := make([]Indexer, len(r.indexers))
	copy(idxCopy, r.indexers)
	r.mu.RUnlock()

	r.statsMu.RLock()
	defer r.statsMu.RUnlock()

	out := make([]IndexerStats, 0, len(idxCopy))
	for _, idx := range idxCopy {
		if tracker, ok := r.stats[idx.Name()]; ok {
			out = append(out, tracker.snapshot(idx.Name(), idx.Enabled()))
		}
	}
	return out
}

// NewDefaultRegistry constructs a Registry pre-loaded with all built-in
// indexers: YTS, EZTV, PirateBay, Leet1337x, Nyaa, TorrentGalaxy, Magnetdl,
// and RARBG (mirror). The Generic RSS indexer is not registered here because
// it requires a caller-supplied URL template.
func NewDefaultRegistry() *Registry {
	r := &Registry{}
	r.Register(NewYTSIndexer())
	r.Register(NewEZTVIndexer())
	r.Register(NewPirateBayIndexer())
	r.Register(NewLeet1337xIndexer())
	r.Register(NewNyaaIndexer())
	r.Register(NewTorrentGalaxyIndexer())
	r.Register(NewMagnetdlIndexer())
	r.Register(NewRARBGIndexer())
	return r
}
