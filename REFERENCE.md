# Sonarr / Radarr Reference Notes

Facts extracted from the Sonarr and Radarr source so we do not need to re-scan it. `PLAN.md` §13 holds the analysis; this file holds the raw details.

- Sonarr: `github.com/Sonarr/Sonarr` at `a2b6054a8f3746d149a811bddea2a8d063efc35e` (2026-10-15)
- Radarr: `github.com/Radarr/Radarr` at `df2faa64c75e6081556d11232dc46523e7264ea1` (2026-10-14)
- Local shallow clones at those SHAs: `~/Sandbox/sonarr` and `~/Sandbox/radarr` (outside this repo; re-clone if missing).
- Both GPL-3.0. Paths below are under `src/NzbDrone.Core/` unless noted.
- To read a file, fetch it at the pinned SHA, for example `https://raw.githubusercontent.com/Sonarr/Sonarr/<sha>/src/NzbDrone.Core/Parser/Parser.cs`, or re-clone with `git clone --depth 1`. Only do this for the big files listed in §14 (parser regexes, naming builder). Everything else is below.

**Verification status.** Sections 2, 3, 4.1–4.4, 5.1–5.5, 8 (settings list), 9.1, 9.2 (token list), 10 (hosts), 11.1 (intervals), and 13 were read from the source. Items marked **(unverified)** were summarized from memory or inferred from file/folder names; read the file listed in §14 before relying on them.

---

## 1. Source map

| Concern | Sonarr / Radarr path |
|---|---|
| Decision engine | `DecisionEngine/` (`DownloadDecisionMaker`, `DownloadDecisionComparer`, `Specifications/`) |
| Release-name parsing | `Parser/Parser.cs`, `QualityParser.cs`, `LanguageParser.cs`, `ReleaseGroupParser.cs`, `SceneChecker.cs`, `ParsingService.cs` |
| Quality | `Qualities/Quality.cs`, `QualityDefinition.cs`, `Profiles/Qualities/` |
| Custom formats | `CustomFormats/` (`Specifications/` one class per condition) |
| Delay / release profiles | `Profiles/Delay/`, `Profiles/Releases/` |
| Indexers | `Indexers/` (`Newznab/`, `Torznab/`, `TorrentRss/`, plus per-site folders) |
| Search | `IndexerSearch/` (search criteria classes, `ReleaseSearchService`) |
| Download clients | `Download/Clients/` (one folder per client), `Download/TrackedDownloads/`, `Download/Pending/` |
| Import | `MediaFiles/EpisodeImport/` (Sonarr) or `MediaFiles/MovieImport/` (Radarr) |
| File ops | `MediaFiles/` (`DiskScanService`, `*FileMovingService`, `RecycleBinProvider`, `UpgradeMediaFileService`) |
| Naming | `Organizer/FileNameBuilder.cs` (~1300 lines), `NamingConfig.cs` |
| Media info | `MediaFiles/MediaInfo/` (`VideoFileInfoReader` calls FFprobe via FFMpegCore) |
| Library | `Tv/` (Sonarr), `Movies/` (Radarr) |
| Metadata | `MetadataSource/SkyHook/SkyHookProxy.cs` |
| Notifications | `Notifications/` |
| Import lists | `ImportLists/` |
| Scheduler / commands | `Jobs/TaskManager.cs`, `Messaging/Commands/` |
| Providers (shared base) | `ThingiProvider/` |
| Health | `HealthCheck/Checks/` |
| DB | `Datastore/` (`TableMapping.cs`, `Migration/`) |
| HTTP API | `src/Sonarr.Api.V3/`, `src/Sonarr.Api.V5/` (with `openapi.json`), `src/Radarr.Api.V3/` |
| UI | `frontend/src/` (React + Vite) |

---

## 2. Database tables

Shared: `Config, RootFolders, ScheduledTasks, Indexers, ImportLists, Notifications, Metadata, DownloadClients, History, QualityDefinitions, CustomFormats, QualityProfiles, Logs, NamingConfig, Blocklist, MetadataFiles, SubtitleFiles, ExtraFiles, PendingReleases, RemotePathMappings, Tags, ReleaseProfiles, DelayProfiles, Users, Commands, IndexerStatus, DownloadClientStatus, ImportListStatus, NotificationStatus, CustomFilters, DownloadHistory, UpdateHistory`

Sonarr only: `Series, SeriesTranslations, Episodes, EpisodeFiles, SceneMappings, ImportListItems, ImportListExclusions, QualityProfileQualityRanks`

Radarr only: `Movies, MovieMetadata, MovieFiles, AlternativeTitles, MovieTranslations, Credits, Collections, ImportListMovies, ImportExclusions`

Notes:
- Radarr separates `Movies` (user's library entry: path, profile, monitored) from `MovieMetadata` (provider data keyed by TMDB id). This lets one metadata row serve import-list items and library entries. Copy this split.
- Provider tables (`Indexers`, `DownloadClients`, `Notifications`, `ImportLists`, `Metadata`) share columns: name, implementation, config contract, JSON settings, tags, enable flags (RSS/search/etc.). `*Status` tables hold `initialFailure, mostRecentFailure, escalationLevel, disabledTill`.
- `History` is event rows keyed by `downloadId` so a grab, its failure and its import can be joined. `DownloadHistory` is a separate, simpler table used for "already imported" checks.
- Logs live in a **separate** database file.

---

## 3. Core models

### 3.1 Series (Sonarr `Tv/Series.cs`)
`tvdbId, tvRageId, tvMazeId, imdbId, tmdbId, malIds[], aniListIds[]`, `title, cleanTitle, sortTitle, titleSlug, overview, status (Continuing|Ended|Upcoming|Deleted), seriesType (Standard|Daily|Anime), network, airTime, runtime, year, certification, genres[], actors[], ratings, images[], originalLanguage, originalTitle`, `monitored, monitorNewItems (All|None), qualityProfileId, seasonFolder, useSceneNumbering, path, rootFolderPath, pendingPath, tags[], added, firstAired, lastAired, lastInfoSync, seasons[] {seasonNumber, monitored}, seasonTypes[], translations[], addOptions`.

### 3.2 Episode (`Tv/Episode.cs`)
`seriesId, tvdbId, episodeFileId, seasonNumber, episodeNumber, title, airDate ("yyyy-MM-dd"), airDateUtc, overview, monitored, absoluteEpisodeNumber, sceneAbsoluteEpisodeNumber, sceneSeasonNumber, sceneEpisodeNumber, unverifiedSceneNumbering, airedAfterSeasonNumber, airedBeforeSeasonNumber, airedBeforeEpisodeNumber, ratings, images[], lastSearchTime, runtime, finaleType`.

### 3.3 Movie (Radarr `Movies/Movie.cs`, `MovieMetadata.cs`)
- `Movie`: `movieMetadataId, monitored, minimumAvailability, qualityProfileId, path, rootFolderPath, added, tags[], addOptions, lastSearchTime, movieFileId`
- `MovieMetadata`: `tmdbId, imdbId, title, cleanTitle, sortTitle, originalTitle, originalLanguage, overview, status, year, secondaryYear, runtime, certification, genres[], keywords[], images[], ratings, studio, website, youTubeTrailerId, collectionTmdbId, collectionTitle, inCinemas, physicalRelease, digitalRelease, alternativeTitles[], translations[], recommendations[], popularity, lastInfoSync`
- `MovieStatusType`: `Deleted=-1, TBA=0, Announced=1, InCinemas=2, Released=3`. Status `InCinemas` means in cinemas < 3 months; `Released` means physical/digital release or > 3 months in cinemas.
- **`IsAvailable(delay)`** (minimum availability): `TBA|Announced` → always available. `InCinemas` → available from the `inCinemas` date if known. Otherwise (`Released`) → the earlier of `physicalRelease` and `digitalRelease`; if neither exists, `inCinemas + 90 days`; if no dates at all, never available. The user's delay (days) is added to the chosen date (not applied when the date is the min/max sentinel). A movie is not searched until `IsAvailable` is true.

### 3.4 ReleaseInfo (`Parser/Model/ReleaseInfo.cs`)
`guid, title, size, downloadUrl, infoUrl, commentUrl, indexerId, indexer, indexerPriority, downloadProtocol (Usenet|Torrent), publishDate, origin, source, container, codec, resolution, languages[], indexerFlags (bitfield), pendingReleaseReason, tvdbId, tvRageId, imdbId` + torrent extras: `infoHash, magnetUrl, seeders, peers, minimumRatio, minimumSeedTime`.

### 3.5 Parsed info
- Sonarr `ParsedEpisodeInfo`: `seriesTitle, seriesTitleInfo, quality (quality+revision), seasonNumber, episodeNumbers[], absoluteEpisodeNumbers[], specialAbsoluteEpisodeNumbers[], airDate, languages[], fullSeason, isPartialSeason, isMultiSeason, isSeasonExtra, isSplitEpisode, isMiniSeries, special, releaseGroup, releaseHash, seasonPart, releaseTokens, daily/anime flags, releaseType (SingleEpisode|MultiEpisode|SeasonPack)`.
- `RemoteEpisode`/`RemoteMovie` = parsed info + matched library item + `ReleaseInfo` + `customFormats[]` + `customFormatScore` + `downloadAllowed`.

### 3.6 Enums to copy
| Enum | Values |
|---|---|
| Sonarr `MonitorTypes` (add flow) | Unknown, All, Future, Missing, Existing, FirstSeason, LastSeason, LatestSeason (obsolete), Pilot, Recent, MonitorSpecials, UnmonitorSpecials, None, Skip |
| `NewItemMonitorTypes` | All, None |
| `SeriesTypes` | Standard=0, Daily=1, Anime=2 |
| `SeriesStatusType` | Deleted=-1, Continuing=0, Ended=1, Upcoming=2 |
| `EpisodeHistoryEventType` | Unknown=0, Grabbed=1, SeriesFolderImported=2, DownloadFolderImported=3, DownloadFailed=4, EpisodeFileDeleted=5, EpisodeFileRenamed=6, DownloadIgnored=7 |
| `MovieHistoryEventType` | Unknown=0, Grabbed=1, DownloadFolderImported=3, DownloadFailed=4, MovieFileDeleted=6, MovieFolderImported=7, MovieFileRenamed=8, DownloadIgnored=9 |
| `DownloadProtocol` | Unknown=0, Usenet=1, Torrent=2 |
| `DownloadItemStatus` | Queued=0, Paused=1, Downloading=2, Completed=3, Failed=4, Warning=5 |
| `TrackedDownloadState` | Downloading, ImportBlocked, ImportPending, Importing, Imported, FailedPending, Failed, Ignored |
| `TrackedDownloadStatus` | Ok, Warning, Error |
| `PendingReleaseReason` | Delay=0, DownloadClientUnavailable=1, Fallback=2 |
| `ProperDownloadTypes` | PreferAndUpgrade, DoNotUpgrade, DoNotPrefer |
| `RescanAfterRefreshType` | Always, AfterManual, Never |
| `FileDateType` | None, LocalAirDate, UtcAirDate |
| `EpisodeTitleRequiredType` | Always, BulkSeasonReleases, Never |
| `ReleaseType` | Unknown, SingleEpisode, MultiEpisode, SeasonPack |
| `ImportMode` | Auto, Move, Copy |
| `CommandPriority` | Low=-1, Normal=0, High=1 |
| `CommandStatus` | Queued, Started, Completed, Failed, Aborted, Cancelled, Orphaned |
| `CommandTrigger` | Unspecified, Manual, Scheduled |
| `RejectionType` | Permanent=0, Temporary=1 |
| `SpecificationPriority` | Default/Parsing/Database=0, Disk=1 |
| `MultiEpisodeStyle` | Extend, Duplicate, Repeat, Scene, Range, PrefixedRange |
| `ColonReplacementFormat` | Delete, Dash, SpaceDash, SpaceDashSpace, Smart, Custom |
| Radarr `IndexerFlags` (bitfield) | G_Freeleech=1, G_Halfleech=2, G_DoubleUpload=4, PTP_Golden=8, PTP_Approved=16, G_Internal=32, AHD_Internal=64, G_Scene=128, G_Freeleech75=256, G_Freeleech25=512, AHD_UserRelease=1024, Nuked=2048. Sonarr has its own (smaller) `IndexerFlags.cs`. |

---

## 4. Quality

### 4.1 Sonarr qualities (id, name, source, resolution)
0 Unknown · 1 SDTV (TV, 480) · 2 DVD (DVD, 480) · 3 WEBDL-1080p · 4 HDTV-720p · 5 WEBDL-720p · 6 Bluray-720p · 7 Bluray-1080p · 8 WEBDL-480p · 9 HDTV-1080p · 10 Raw-HD (TV raw, 1080) · 12 WEBRip-480p · 13 Bluray-480p · 14 WEBRip-720p · 15 WEBRip-1080p · 16 HDTV-2160p · 17 WEBRip-2160p · 18 WEBDL-2160p · 19 Bluray-2160p · 20 Bluray-1080p Remux (Bluray raw) · 21 Bluray-2160p Remux · 22 Bluray-576p.
Sources: Unknown, Television, TelevisionRaw, Web, WebRip, DVD, Bluray, BlurayRaw. Radarr adds Workprint, CAM, Telesync, Telecine, Regional, DVDSCR, DVD-R, BR-DISK and uses ids that differ from Sonarr's; keep ours as a single superset table with a stable id per quality.

### 4.2 Quality model
`QualityModel { quality, revision { version, real, isRepack } }`. Comparison: quality position in the profile, then revision (`real` outranks `version`).

### 4.3 Default size limits (MB per minute of runtime)
Sonarr (Weight / Min / Max / Preferred): Unknown 1/1/199.9/95 · SDTV 2/2/100/95 · WEB 480p 3/2/100/95 · DVD 4/2/100/95 · Bluray-480p 5/2/100/95 · Bluray-576p 6/2/100/95 · HDTV-720p 7/3/125/95 · HDTV-1080p 8/4/125/95 · Raw-HD 9/4/∞/95 · WEB 720p 10/3/130/95 · Bluray-720p 11/4/130/95 · WEB 1080p 12/4/130/95 · Bluray-1080p 13/4/155/95 · Bluray-1080p Remux 14/35/∞/95 · HDTV-2160p 15/35/199.9/95 · WEB 2160p 16/35/∞/95 · Bluray-2160p 17/35/∞/95 · Bluray-2160p Remux 18/35/∞/95.
Radarr: min 0 everywhere; max 100 up to WEB 1080p, unlimited above; preferred 95 up to WEB 1080p, none above. Weights 1–26 in the order: Unknown, Workprint, CAM, Telesync, Telecine, Regional, DVDSCR, SDTV, DVD, DVD-R, WEB 480p, Bluray-480p, Bluray-576p, HDTV-720p, WEB 720p, Bluray-720p, HDTV-1080p, WEB 1080p, Bluray-1080p, Remux-1080p, HDTV-2160p, WEB 2160p, Bluray-2160p, Remux-2160p, BR-DISK, Raw-HD.
- Qualities that share a `GroupName` (WEBDL + WEBRip of the same resolution) share a weight and rank together.
- `Quality profile`: ordered list of items, each a quality or a named group; `cutoff` (item id); `upgradeAllowed`; custom-format list with scores (`formatItems`), `minFormatScore`, `cutoffFormatScore`, `minUpgradeFormatScore`; Sonarr also has `language`.

### 4.4 Custom-format conditions
Radarr: Edition, IndexerFlag, Language, QualityModifier, ReleaseGroup, ReleaseTitle, Resolution, Size, Source, Year. Sonarr: IndexerFlag, Language, ReleaseGroup, ReleaseTitle, ReleaseType, Resolution, Size, Source. Each condition has `negate` and `required` flags. Conditions are grouped by type (`SpecificationMatchesGroup`): a group matches when no `required` condition in it failed **and** at least one condition in it matched; a custom format matches when **all** groups match. A release's score is the sum of the profile scores of every format it matches.

---

## 5. Decision engine detail

### 5.1 Pipeline (`DownloadDecisionMaker`)
1. For each `ReleaseInfo`: parse title → match to library item (Sonarr: series via `ParsingService` using title, alt titles, scene mappings, TVDB id; Radarr: movie via title + year + alt titles + IMDb/TMDB id).
2. Build `RemoteEpisode`/`RemoteMovie`, compute custom formats and score.
3. Run specs in priority order (`SpecificationPriority`); stop on the first **Permanent** rejection; collect **Temporary** rejections.
4. Result: approved / rejected(permanent) / pending(temporary).
5. Approved list → `DownloadDecisionPriorizationService` sorts with the comparer in §5.3, groups by episode/movie so one grab is made per target, then `ProcessDownloadDecisions` sends to the download client (or stores as pending if `PendingReleaseReason` is set).

### 5.2 Rejection reasons (use as our reason codes)
Sonarr `DownloadRejectionReason`:
`Unknown, UnknownSeries, UnknownEpisode, MatchesAnotherSeries, UnableToParse, Error, DecisionError, MinimumAgeDelay, MinimumAgeDelayPushed, SeriesNotMonitored, EpisodeNotMonitored, History{RecentCutoffMet, CdhDisabledCutoffMet, HigherPreference, HigherRevision, CutoffMet, CustomFormatCutoffMet, CustomFormatScore, CustomFormatScoreIncrement, UpgradesNotAllowed}, NoMatchingTag, PropersDisabled, ProperForOldFile, WrongEpisode, WrongSeason, WrongSeries, FullSeason, UnknownRuntime, BelowMinimumSize, AboveMaximumSize, AlreadyImportedSameHash, AlreadyImportedSameName, UnknownReleaseGroup, ReleaseGroupDoesNotMatch, IndexerDisabled, Blocklisted, CustomFormatMinimumScore, MinimumFreeSpace, FullSeasonNotAired, MaximumSizeExceeded, MinimumAge, MaximumAge, MultiSeason, Sample, ProtocolDisabled, QualityNotWanted, QualityUpgradesDisabled, Queue{HigherPreference, HigherRevision, CutoffMet, CustomFormatCutoffMet, CustomFormatScore, CustomFormatScoreIncrement, UpgradesNotAllowed, PropersDisabled}, Raw, MustContainMissing, MustNotContainPresent, RepackDisabled, RepackUnknownReleaseGroup, RepackReleaseGroupDoesNotMatch, ExistingFileHasMoreEpisodes, AmbiguousNumbering, NotSeasonPack, SplitEpisode, MinimumSeeders, Disk{HigherPreference, HigherRevision, CutoffMet, CustomFormatCutoffMet, CustomFormatScore, CustomFormatScoreIncrement, UpgradesNotAllowed, NotUpgrade}, BeforeAirDate`.
Radarr differs by: `UnknownMovie, Availability, MovieNotMonitored, WrongMovie, HardcodeSubtitles, WantedLanguage, RequiredFlags`; no series/season/episode reasons.

Import-side `ImportRejectionReason` — Sonarr: `Unknown, FileLocked, UnknownSeries, DangerousFile, ExecutableFile, UserRejectedExtension, ArchiveFile, SeriesFolder, InvalidFilePath, UnsupportedExtension, PartialSeason, SeasonExtra, InvalidSeasonOrEpisode, UnableToParse, Error, DecisionError, NoEpisodes, MissingAbsoluteEpisodeNumber, EpisodeAlreadyImported, TitleMissing, TitleTba, MinimumFreeSpace, FullSeason, NoAudio, EpisodeUnexpected, EpisodeNotFoundInRelease, Sample, SampleIndeterminate, Unpacking, ExistingFileHasMoreEpisodes, SplitEpisode, UnverifiedSceneMapping, NotQualityUpgrade, NotRevisionUpgrade, NotCustomFormatUpgrade, NotCustomFormatUpgradeAfterRename, MultiSeason`. Radarr: `Unknown, FileLocked, UnknownMovie, DangerousFile, ExecutableFile, ArchiveFile, MovieFolder, InvalidFilePath, UnsupportedExtension, InvalidMovie, UnableToParse, Error, DecisionError, MovieAlreadyImported, MinimumFreeSpace, NoAudio, MovieNotFoundInRelease, Sample, SampleIndeterminate, Unpacking, MultiPartMovie, NotQualityUpgrade, NotRevisionUpgrade, NotCustomFormatUpgrade`.

### 5.3 Ranking order (first difference wins) — verified
**Radarr** (`DownloadDecisionComparer`, ported to `omp-decision/src/compare.rs`):
1. **Quality**: index in the profile; then revision, unless propers are set to `DoNotPrefer`.
2. **Custom-format score** (higher).
3. **Protocol**: the release matching the delay profile's `PreferredProtocol` wins.
4. **Indexer priority**: lower number wins.
5. **Indexer flags**, only when the `PreferIndexerFlags` setting is on: +2 each for DoubleUpload, Freeleech, PTP Approved, PTP Golden, Internal; +1 for Halfleech.
6. **Torrent only**: `round(log10(seeders))`, then `round(log10(peers))` (round half to even, so 90 and 110 seeders tie).
7. **Usenet only**, age bucket: < 1 h → 1000; ≤ 24 h → 100; ≤ 7 days → 10; older → `-round(log10(days))`.
8. **Size**: with a `preferredSize` and a known runtime, the release closest to `runtime × preferredSize` wins (distance truncated to a multiple of 200 MB); otherwise bigger wins (size truncated to 200 MB).

**Sonarr** adds, after protocol: episode count (full-season pack first; for anime more episodes wins, otherwise fewer), then lowest episode number, before indexer priority; and it has no indexer-flags step.

### 5.4 Delay logic (`DelaySpecification`, priority Database, **Temporary**) — verified
Radarr's version (ported to `omp-decision/src/specs/upgrade.rs`):
1. Accept if the search was user-invoked.
2. Pick the delay profile for the movie's tags; `delay` = that profile's delay for the release's protocol; `preferred` = release protocol equals the profile's preferred protocol.
3. Accept if `delay == 0`.
4. If `preferred` and the movie has a file: accept when `IsUpgradable(file, release)` is `None` **and** the release is a revision upgrade of the file (a proper that really upgrades never waits).
5. Accept if `BypassIfHighestQuality`, `preferred`, and the release quality ranks ≥ the last allowed quality in the profile.
6. Accept if `BypassIfAboveCustomFormatScore`, `preferred`, and score ≥ `MinimumCustomFormatScore`.
7. Accept if the oldest pending release for the movie is already older than `delay` minutes.
8. Reject (`MinimumAgeDelay`, "Waiting for better quality release") if the release is younger than `delay`; otherwise accept.
Sonarr's differs in step 4: it accepts a same-quality higher-revision release when propers are set to `PreferAndUpgrade`, without calling `IsUpgradable`.

### 5.5 Upgrade rules (`UpgradableSpecification`) — verified, ported to `omp-quality/src/upgrade.rs`
`IsUpgradable(profile, current, new)` returns a reason; `None` means "upgrade". Checks in order:
1. New quality ranks higher **and** `QualityCutoffNotMet(profile, current, new)` → `None`.
2. New quality ranks lower → `BetterQuality`.
3. Propers setting is not `DoNotPrefer` and new revision > current → `None`.
4. Profile `upgradeAllowed` is false → `UpgradesNotAllowed`.
5. Propers setting is not `DoNotPrefer` and new revision < current → `BetterRevision`.
6. New quality ranks higher (cutoff already met) → `QualityCutoff`.
7. `newScore <= currentScore` → `CustomFormatScore`.
8. `currentScore >= cutoffFormatScore` → `CustomFormatCutoff`.
9. `newScore < currentScore + minUpgradeFormatScore` → `MinCustomFormatScore`.
10. otherwise `None`.

Helpers:
- `QualityCutoffNotMet`: cutoff = `profile.cutoff` if upgrades allowed, else the **first allowed quality**. True if current ranks below the cutoff, or if `new` is a revision upgrade of `current`.
- `CustomFormatCutoffNotMet`: score < (`cutoffFormatScore` if upgrades allowed, else `minFormatScore`).
- `CutoffNotMet` = either of the above.
- `IsRevisionUpgrade`: **same quality** (not just same rank) and revision compares greater (`real` first, then `version`).
- `IsUpgradeAllowed`: true if revision upgrade; else if quality or format-score upgrade, returns `profile.upgradeAllowed`; else true (non-upgrades are not this rule's business).
- Profile quirk: `GetIndex` returns index 0 for a quality the profile does not mention, so unknown qualities rank lowest. Group ids (> 0) can be used as the cutoff and resolve to the group's row.
- Custom-format match: conditions are grouped **by kind**; a kind passes when no `required` condition in it failed and at least one in it matched; the format matches when all kinds pass. Language conditions apply `negate` themselves; "Original" resolves to the movie's original language when known.

---

## 6. Search

- **Search criteria types.** Sonarr: `SingleEpisode, Season, DailyEpisode, DailySeason, AnimeEpisode, AnimeSeason, SpecialEpisode`. Radarr: `Movie`. Each indexer builds requests from the criteria (Newznab params `tvdbid, season, ep, q` / `imdbid, tmdbid, q`; anime uses absolute numbers).
- **Season search** **(unverified)**: tries a season pack first; `SeasonSearchMaximumSingleEpisodeAge` (on `ReleaseInfo`) and the `SeasonPackUpgrade`/`SeasonPackUpgradeThreshold` settings control pack preference.
- **RSS sync** fetches the recent feed of every RSS-enabled indexer, runs every item through the decision engine against all monitored targets, grabs approved ones. Minimum interval 10 minutes (setting `RssSyncInterval`).
- **Wanted/Missing** **(unverified details)** = monitored, aired (or available), no file. **Cutoff Unmet** = has file below cutoff (quality or format score). Both are searchable in bulk; `LastSearchTime` is stored per item to throttle.
- **Newznab/Torznab**: Sonarr default categories `5030, 5040` (TV SD/HD) plus anime categories; Radarr uses the `2000` movie category family **(unverified)**. Indexer settings: URL, API path (`/api`), API key, categories, anime categories, additional params, multi-language, minimum seeders, seed ratio, seed time, season-pack seed time, RSS/automatic search/interactive search toggles, priority (1–50, default 25), download-client override, tags.
- **Indexer request flow**: `IIndexerRequestGenerator` builds a *pageable request chain* (tiers; later tiers only run if earlier return nothing) → HTTP → `IParseIndexerResponse` → `ReleaseInfo[]`. Plugin interface should mirror this: `generate_requests(criteria) → request chains` and `parse(response) → releases` so the host does HTTP, rate-limiting, and back-off.
- **Provider status/back-off** (`EscalationBackOff.Periods`, seconds, by consecutive-failure level): 0, 60, 300, 900, 1800, 3600, 10 800, 21 600, 43 200, 86 400. Failures raise the level and set `disabledTill = now + period`; success resets the level. The same table applies to indexers, download clients, import lists and notifications.

---

## 7. Download clients

`IDownloadClient` (what plugin download clients must offer):
- `Download(release, indexer) → downloadId`
- `GetItems() → [DownloadClientItem]` (called by the 1-minute monitor)
- `GetFiles(item)`, `GetImportItem(item, previousAttempt)` (client may need to expose the final path)
- `RemoveItem(item, deleteData)`, `MarkItemAsImported(item)` (set post-import category)
- `GetStatus() → { isLocalhost, outputRootFolders[], removesCompletedDownloads }`

`DownloadClientItem`: `downloadId, category, title, totalSize, remainingSize, remainingTime, seedRatio, outputPath, message, status, isEncrypted, canMoveFiles, canBeRemoved`, plus client info (protocol, type, id, name, `removeCompletedDownloads`, `hasPostImportCategory`).

Settings that matter: per-client category (tv/movie), post-import category, recent/older priority, initial state (paused/force), remove-completed and remove-failed toggles, `EnableCompletedDownloadHandling`, `AutoRedownloadFailed` (+ from interactive search), `DownloadClientHistoryLimit`, `DownloadClientWorkingFolders` (`_UNPACK_|_FAILED_`), **remote path mappings** (host, remote path, local path).

Import mechanics **(unverified)**: `Auto` = hardlink/copy when the torrent must keep seeding, move otherwise; `Move` and `Copy` are explicit. Read `MediaFiles/*FileMovingService.cs` and `Download/CompletedDownloadService.cs` before implementing. `CopyUsingHardlinks` setting. The torrent seed goal (ratio/time) decides when `canBeRemoved` becomes true.

Health checks tied to clients: client unavailable, client removes completed downloads (breaks import), client sorting enabled (breaks category paths), root folder inside the client's download folder, remote path mapping wrong.

---

## 8. Import pipeline

1. **Trigger**: tracked download becomes `ImportPending`, or manual import, or disk scan of a root folder.
2. **Scan** files under the output path; filter by `MediaFileExtensions` and `UserRejectedExtensions`; reject archives, executables ("dangerous files").
3. **Parse** each file name (and the folder / grabbed release name as context); match to library item (`ImportDecisionMaker`).
4. **Specs** (per file): Sonarr — AbsoluteEpisodeNumber, AlreadyImported, EpisodeTitle (required type), FreeSpace, FullSeason, HasAudioTrack, MatchesFolder, MatchesGrab, NotSample, NotUnpacking, SameEpisodesImport, SplitEpisode, UnverifiedSceneNumbering, Upgrade. Radarr — AlreadyImported, FreeSpace, GrabbedReleaseQuality, HasAudioTrack, MatchesFolder, MatchesGrab, NotMultiPart, NotSample, NotUnpacking, Upgrade.
5. **Approved files**: read media info (FFprobe), build the destination path from the naming config, create folders, move/hardlink/copy, set file date and permissions (`ChmodFolder`, `ChownGroup`, `SetPermissionsLinux`), upgrade → old file goes to the recycle bin (`RecycleBin`, `RecycleBinCleanupDays`) or is deleted, write DB rows, write metadata/extra files (`ImportExtraFiles`, `ExtraFileExtensions`), write history, emit `DownloadFolderImported`/`EpisodeImported` events.
6. **Rejected files** keep the tracked download in `ImportBlocked` with the rejection messages visible in the Queue UI; the user can run **Manual Import** to override.
7. **After import**: optionally remove from client; `DeleteEmptyFolders`; notifications; media-server update queue (batched).

Media-management settings (Sonarr `ConfigService`, Radarr is nearly identical): `AutoUnmonitorPreviouslyDownloadedEpisodes, RecycleBin, RecycleBinCleanupDays, DownloadPropersAndRepacks (ProperDownloadTypes), CreateEmptySeriesFolders, DeleteEmptyFolders, FileDate, RescanAfterRefresh, SetPermissionsLinux, ChmodFolder, ChownGroup, EpisodeTitleRequired, SkipFreeSpaceCheckWhenImporting, MinimumFreeSpaceWhenImporting, CopyUsingHardlinks, UseScriptImport, ScriptImportPath, ImportExtraFiles, ExtraFileExtensions, EnableMediaInfo, UserRejectedExtensions`.
Indexer config: `MinimumAge, Retention, MaximumSize, RssSyncInterval`. Download-client config: `EnableCompletedDownloadHandling, AutoRedownloadFailed, AutoRedownloadFailedFromInteractiveSearch, DownloadClientWorkingFolders`. UI config: `UILanguage, FirstDayOfWeek, CalendarWeekColumnHeader, ShortDateFormat, LongDateFormat, TimeFormat, TimeZone, ShowRelativeDates, EnableColorImpairedMode`. Import-list config: `ListSyncLevel, ListSyncTag`.

---

## 9. Naming

### 9.1 Defaults
- Sonarr: standard `{Series Title} - S{season:00}E{episode:00} - {Episode Title} {Quality Full}`; daily `{Series Title} - {Air-Date} - {Episode Title} {Quality Full}`; anime `{Series Title} - S{season:00}E{episode:00} - {Episode Title} {Quality Full}`; series folder `{Series Title}`; season folder `Season {season}`; specials folder `Specials`; colon replacement `Smart`.
- Radarr: movie folder `{Movie Title} ({Release Year})`; file `{Movie Title} ({Release Year}) {Quality Full}`; colon replacement `Smart`.

### 9.2 Token grammar
Token regex (from `FileNameBuilder`): `{<prefix chars>token[ separator word][:format]<suffix chars>}` with `{{`/`}}` as escaped braces. Prefix/suffix are separator characters (`- ._[(` / `- ._)]`) that are dropped when the token is empty. Case of the token text selects output casing (`{series title}` lower, `{SERIES TITLE}` upper, `{Series Title}` as-is, `{Series.Title}` dotted, `{Series_Title}`, `{Series-Title}`). Numeric formats `{season:00}`, `{episode:00}`; truncation `{Episode Title:30}` / `{Series Title:30}` (negative truncates from the left).
Token list, **verified** from the string literals in each `FileNameBuilder`:
- **Sonarr:** `{Series Title}`, `{Series TitleYear}`, `{Series TitleWithoutYear}`, `{Series TitleThe}`, `{Series TitleTheYear}`, `{Series TitleTheWithoutYear}`, `{Series CleanTitle}`, `{Series CleanTitleYear}`, `{Series CleanTitleWithoutYear}`, `{Series CleanTitleThe}`, `{Series CleanTitleTheYear}`, `{Series CleanTitleTheWithoutYear}`, `{Series TitleFirstCharacter}`, `{Series Year}`, `{ImdbId}`, `{TmdbId}`, `{TvdbId}`, `{TvMazeId}`, `{Season}`, `{Season Title}`, `{Season CleanTitle}`, `{Episode}`, `{Episode Title}`, `{Episode CleanTitle}`, `{Air Date}`, `{absolute}`, `{Quality Full}`, `{Quality Title}`, `{Quality Proper}`, `{Quality Real}`, `{Release Group}`, `{Release Hash}`, `{Original Title}`, `{Original Filename}`, `{Custom Formats}`, `{Custom Format:<name>[,<name>]}`, `{ellipsis}` (truncation marker), and MediaInfo: `{MediaInfo Simple}`, `{MediaInfo Full}`, `{MediaInfo Video}`, `{MediaInfo VideoCodec}`, `{MediaInfo VideoBitDepth}`, `{MediaInfo VideoDynamicRange}`, `{MediaInfo VideoDynamicRangeType}`, `{MediaInfo Audio}`, `{MediaInfo AudioCodec}`, `{MediaInfo AudioChannels}`, `{MediaInfo AudioLanguages}`, `{MediaInfo AudioLanguagesAll}`, `{MediaInfo SubtitleLanguages}`, `{MediaInfo SubtitleLanguagesAll}`.
- **Radarr:** `{Movie Title}`, `{Movie TitleThe}`, `{Movie CleanTitle}`, `{Movie CleanTitleThe}`, `{Movie OriginalTitle}`, `{Movie CleanOriginalTitle}`, `{Movie TitleFirstCharacter}`, `{Movie Collection}`, `{Movie CollectionThe}`, `{Movie CleanCollectionThe}`, `{Movie Certification}`, `{Release Year}`, `{Edition Tags}`, `{ImdbId}`, `{TmdbId}`, the same Quality/Release/Original/Custom Format tokens, and the same MediaInfo tokens plus `{MediaInfo 3D}`.
- Multi-episode patterns use `SeasonEpisodePatternRegex` to find the `S{season}E{episode}` block and repeat/extend/range per `MultiEpisodeStyle` (e.g. `S01E01-E02`, `S01E01E02`, `1x01-02`).
- Illegal characters: replaced per OS rules; colons per `ColonReplacementFormat`; trailing dots/spaces trimmed; max path length enforced by truncating the episode title first.
- Validation: pattern must contain a season+episode identifier (Sonarr) or title (Radarr) to be unique; previews computed from sample data.

---

## 10. Metadata servers and APIs

| Host | Used for |
|---|---|
| `skyhook.sonarr.tv/v2/tvdb/{route}/` | Sonarr series info (`shows/{lang}/{tvdbId}`), search (`search/{lang}/?term=`) |
| `services.sonarr.tv/v1/` | scene mappings, update info, misc |
| `api.radarr.video/v1/{route}` | Radarr movie details, search, lists (popular, trending, TMDB lists, IMDb lists, collections) |
| `radarr.servarr.com/v1/` | Radarr services (update, etc.) |
- Both clients pass the user's preferred metadata language. Movie lookups accept `tmdb:`/`imdb:` prefixes in the search term. 404 → "not found" (not an error).
- Treat both as "metadata provider" implementations behind one trait; cache responses with TTL and keep stale copies for outage tolerance.

---

## 11. Scheduler, commands, events

### 11.1 Default scheduled tasks (minutes)
| Task | Sonarr | Radarr |
|---|---|---|
| RefreshMonitoredDownloads | 1 (High priority) | setting (default 1) |
| MessagingCleanup | 5 | 5 |
| ClearExpiredCache | 5 | 5 |
| ImportListSync | 5 | 5 |
| RssSync | setting (min 10) | setting (min 10) |
| UpdateSceneMapping | 180 | – |
| ApplicationUpdateCheck | 360 | 360 |
| CheckHealth | 360 | 360 |
| RefreshSeries / RefreshMovie | 720 | 1440 |
| RefreshCollections | – | 1440 |
| Housekeeping | 1440 | 1440 |
| CleanUpRecycleBin | 1440 | 1440 |
| Backup | setting | setting |
- On startup, task rows are reconciled with defaults: obsolete rows removed, intervals/priorities overwritten from code, `LastExecution` kept. Running a task updates `LastExecution`/`LastStartTime`. Tasks only run if `interval > 0` and `lastExecution + interval < now`.

### 11.2 Commands (user-triggered or internal)
Refresh (series/movie/all), RescanSeries/RescanMovie, EpisodeSearch / SeasonSearch / SeriesSearch / MovieSearch / MissingEpisodeSearch / CutoffUnmetEpisodeSearch / MissingMoviesSearch / CutoffUnmetMoviesSearch, RenameFiles / RenameSeries / RenameMovies, DownloadedEpisodesScan / DownloadedMoviesScan (manual import), ProcessMonitoredDownloads, RefreshMonitoredDownloads, CheckForFinishedDownload, RssSync, ImportListSync, Backup, ApplicationUpdate(Check), CheckHealth, Housekeeping, CleanUpRecycleBin, ClearBlocklist, DeleteLogFiles, DeleteUpdateLogFiles, MessagingCleanup, ClearExpiredCache. Commands are deduplicated by body equality while queued, and have `CommandPriority`.

### 11.3 Events (the bus)
Grab: `EpisodeGrabbedEvent`/`MovieGrabbedEvent`. Download: `DownloadCompletedEvent`, `DownloadFailedEvent`, `DownloadIgnoredEvent`, `DownloadsProcessedEvent`, `ManualInteractionRequiredEvent`, `UntrackedDownloadCompletedEvent`, `TrackedDownloadRefreshedEvent`. Library: series/movie added, updated, deleted, refreshed, scanned; episode file added/deleted/renamed. Provider: `*UpdatedEvent`, status changes. System: health changed, update available, RSS sync complete, `PendingReleasesUpdatedEvent`. Handlers can declare an ordering (`EventHandleOrder`).

---

## 12. API surface (V3) — use as a checklist for our native API

**Resources (both) — names taken from controller folders and `V3ApiController(...)` attributes; sub-routes like `/schema`, `/test`, `/bulk` and `system/*` beyond backup/task are (unverified), diff against `openapi.json` (§14):** `alttitle`(R), `autotagging`, `blocklist`, `calendar` (+ `.ics` feed), `command`, `config/{downloadclient,host,importlist,indexer,mediamanagement,metadata,naming,ui}`, `customfilter`, `customformat`, `delayprofile`, `diskspace`, `downloadclient` (+ `/schema`, `/test`, `/testall`, `/action/{name}`, `/bulk`), `exclusions`/`importlistexclusion`, `filesystem`, `health`, `history` (+ `/series` or `/movie`), `importlist`, `indexer`, `localization`, `log`, `log/file`, `manualimport`, `mediacover`, `metadata`, `notification`, `parse`, `qualitydefinition`, `qualityprofile` (+ `/schema`), `queue` (+ `/details`, `/status`, `/grab`, `/bulk`), `release` (search results; POST to grab), `release/push`, `remotepathmapping`, `rename`, `rootfolder`, `system/{status,backup,task,restart,shutdown,routes}`, `tag` (+ `/detail`), `update`, `wanted/missing`, `wanted/cutoff`, `releaseprofile`.
**Sonarr only:** `series` (+ `/editor`, `/import`, `/lookup`), `episode`, `episodefile` (+ `/bulk`), `seasonpass`, `languageprofile/schema`.
**Radarr only:** `movie` (+ `/editor`, `/import`, `/lookup`, `/bulk`), `moviefile` (+ `/editor`, `/bulk`), `collection`, `credit`, `extrafile`, `importlist/movie`.
**Auth/realtime:** `X-Api-Key` header or `apikey` query; SignalR hub for live updates (queue, commands, health, library changes).
Sonarr ships an `openapi.json` for V3 and V5 in-tree (useful to diff against our own spec).

---

## 13. Health checks — verified from `HealthCheck/Checks/`
Shared: AllowedHosts, ApiKeyValidation, AppDataLocation, DownloadClient, DownloadClientRemovesCompletedDownloads, DownloadClientRootFolder, DownloadClientSorting, DownloadClientStatus, ImportListRootFolder, ImportListStatus, ImportMechanism, IndexerDownloadClient, IndexerJackettAll, IndexerLongTermStatus, IndexerRss, IndexerSearch, IndexerStatus, Mount, NotificationStatus, PackageGlobalMessage, Proxy, RecyclingBin, RemotePathMapping, RootFolder, SystemTime, Update.
Sonarr only: OidcConfiguration, PasswordResetToken, RemovedSeries.
Radarr only: Metadata, MovieCollectionRootFolder, NamingConfig, PTPOldSettings, ReleaseBranch, RemovedMovie, SlackUrl, SqliteVersion.

---

## 14. Files still worth reading directly when we implement them
These are too large or too exact to summarize; read them at the pinned SHA when the matching work starts.
| Work item | File | Why |
|---|---|---|
| Parser (TV) | `Parser/Parser.cs` (~149 regexes) | Exact patterns and their order matter |
| Parser (movies) | Radarr `Parser/Parser.cs` (44 regexes) | Same |
| Quality detection | `Parser/QualityParser.cs` (~790 lines) | Source/resolution/revision rules |
| Languages | `Parser/LanguageParser.cs`, `Languages/` | Language tokens and ISO map |
| Release groups | `Parser/ReleaseGroupParser.cs` | Edge cases |
| Naming | `Organizer/FileNameBuilder.cs` | Token handlers, truncation, multi-episode |
| Title matching | `Parser/ParsingService.cs` | How a parsed title maps to a library item |
| Upgrade logic | `DecisionEngine/Specifications/UpgradableSpecification.cs`, `UpgradeDiskSpecification.cs` | Exact comparisons |
| Delay | `DecisionEngine/Specifications/RssSync/DelaySpecification.cs` | Exact rules |
| Tests as spec | Sonarr `NzbDrone.Core.Test/ParserTests/` (31 files), `OrganizerTests/`, `DecisionEngineTests/` | Input→output cases to reuse (GPL allows) |
| API shapes | `src/Sonarr.Api.V3/openapi.json`, `src/Sonarr.Api.V5/openapi.json` | Field names/types |
