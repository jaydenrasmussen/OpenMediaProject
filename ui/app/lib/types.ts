// Shapes returned by /api/v1. Kept in step with crates/omp-app/src/api.rs.

export interface Cover {
  kind: "Poster" | "Fanart" | string;
  url: string;
}

export interface MovieFile {
  id: number;
  relative_path: string;
  size: number;
  date_added: string;
  quality: string;
  release_group: string | null;
  scene_name: string | null;
  edition: string | null;
}

export interface Movie {
  id: number;
  tmdb_id: number;
  imdb_id: string | null;
  title: string;
  original_title: string;
  year: number;
  overview: string | null;
  runtime: number;
  status: string;
  genres: string[];
  certification: string | null;
  collection: string | null;
  in_cinemas: string | null;
  digital_release: string | null;
  physical_release: string | null;
  images: Cover[];
  monitored: boolean;
  minimum_availability: string;
  quality_profile_id: number;
  path: string;
  root_folder_path: string | null;
  tags: number[];
  added: string;
  last_search_time: string | null;
  has_file: boolean;
  file: MovieFile | null;
}

export interface LookupResult {
  tmdb_id: number;
  imdb_id: string | null;
  title: string;
  year: number;
  overview: string | null;
  runtime: number;
  images: Cover[];
  in_library: number | null;
}

export interface Release {
  id: string;
  guid: string;
  title: string;
  indexer: string;
  protocol: string;
  size: number;
  age_hours: number;
  seeders: number | null;
  peers: number | null;
  quality: string | null;
  release_group: string | null;
  custom_format_score: number;
  matched_movie_id: number | null;
  approved: boolean;
  temporarily_rejected: boolean;
  rejections: string[];
  rank: number | null;
}

export interface ReleaseSearch {
  releases: Release[];
  indexer_failures: { indexer: string; error: string }[];
}

export interface QueueItem {
  download_id: string;
  title: string;
  movie_id: number | null;
  state: string;
  status: string;
  client: string;
  protocol: string;
  indexer: string | null;
  size: number;
  size_left: number;
  added: string | null;
  warnings: string[];
  message: string | null;
}

export interface HistoryItem {
  id: number;
  movie_id: number;
  event: string;
  date: string;
  source_title: string;
  quality: string;
  download_id: string | null;
  data: Record<string, string>;
}

export interface Provider {
  id: number;
  name: string;
  implementation: string;
  plugin_id: string | null;
  settings: Record<string, unknown>;
  tags: number[];
  enabled: boolean;
  enable_rss: boolean;
  enable_automatic_search: boolean;
  enable_interactive_search: boolean;
  priority: number;
}

export interface RootFolder {
  id: number;
  path: string;
}

export interface ProfileItem {
  Quality?: { quality: number; allowed: boolean };
  Group?: { id: number; name: string; qualities: number[]; allowed: boolean };
}

export interface QualityProfile {
  id: number;
  name: string;
  cutoff: number;
  items: ProfileItem[];
  min_format_score: number;
  cutoff_format_score: number;
  min_upgrade_format_score: number;
  format_items: { format_id: number; score: number }[];
  language: unknown;
  upgrade_allowed: boolean;
}

export interface ImportConfig {
  mode: "auto" | "move" | "copy";
  copy_using_hardlinks: boolean;
  minimum_free_space_mb: number;
  skip_free_space_check: boolean;
  download_propers: boolean;
  working_folders: string;
  naming: {
    rename_movies: boolean;
    replace_illegal_characters: boolean;
    colon_replacement: "delete" | "dash" | "space_dash" | "space_dash_space" | "smart";
    standard_movie_format: string;
    movie_folder_format: string;
  };
}

export interface AuthStatus {
  setup_required: boolean;
  authenticated: boolean;
}

export interface HealthIssue {
  source: string;
  level: "warning" | "error";
  message: string;
}

export interface SystemStatus {
  version: string;
  started: string;
  tmdb_configured: boolean;
  data_dir: string;
}

export interface BackupInfo {
  name: string;
  size: number;
  created: string;
}

export interface Tag {
  id: number;
  label: string;
}

export interface DelayProfile {
  id: number;
  enable_usenet: boolean;
  enable_torrent: boolean;
  preferred_protocol: "usenet" | "torrent";
  usenet_delay: number;
  torrent_delay: number;
  order: number;
  bypass_if_highest_quality: boolean;
  bypass_if_above_custom_format_score: boolean;
  minimum_custom_format_score: number;
  tags: number[];
}

export interface ReleaseProfile {
  id: number;
  name: string;
  enabled: boolean;
  required: string[];
  ignored: string[];
  indexer_id: number;
  tags: number[];
}

export type Condition =
  | { implementation: "release_title" | "edition" | "release_group"; value: string }
  | { implementation: "language"; value: number; except_language: boolean }
  | { implementation: "indexer_flag" | "source" | "resolution" | "quality_modifier"; value: number }
  | { implementation: "size"; min: number; max: number }
  | { implementation: "year"; min: number; max: number };

export type Specification = { name: string; negate: boolean; required: boolean } & Condition;

export interface CustomFormat {
  id: number;
  name: string;
  include_when_renaming: boolean;
  specifications: Specification[];
}

export interface QualityDefinition {
  quality_id: number;
  title: string;
  weight: number;
  min_size: number | null;
  max_size: number | null;
  preferred_size: number | null;
}

export interface DecisionConfig {
  propers_and_repacks: "prefer_and_upgrade" | "do_not_upgrade" | "do_not_prefer";
  minimum_age: number;
  retention: number;
  maximum_size: number;
  enable_completed_download_handling: boolean;
  skip_free_space_check_when_importing: boolean;
  minimum_free_space_when_importing: number;
  allow_hardcoded_subs: boolean;
  whitelisted_hardcoded_subs: string;
  availability_delay: number;
  prefer_indexer_flags: boolean;
}

export interface BlocklistItem {
  id: number;
  movie_id: number;
  movie_title: string;
  source_title: string;
  quality: string;
  protocol: string;
  indexer: string | null;
  size: number | null;
  message: string | null;
  date: string;
}

export interface PluginInfo {
  id: string;
  name: string;
  version: string;
  kind: "indexer" | "notification" | "list";
  description: string;
  allowed_hosts: string[];
}

export interface PluginList {
  plugins: PluginInfo[];
  problems: string[];
}

/** The part of JSON Schema plugins use for their settings forms. */
export interface SchemaProperty {
  type?: "string" | "integer" | "number" | "boolean";
  title?: string;
  description?: string;
  format?: string;
  enum?: (string | number)[];
  default?: unknown;
}

export interface Schema {
  required?: string[];
  properties?: Record<string, SchemaProperty>;
}
