# Feature audit (PLAN §4)

Each line names the tests that show the feature works. `scripts/check_audit.py` checks that every named test exists, and, given the output of `cargo test --workspace`, that each one passed. CI runs it both ways. "e2e" means `e2e/smoke.spec.ts`, run in a real browser against the container.

## 4.1 Library management
- Add, edit, delete; monitored flags: `movies_can_be_added_edited_and_removed`, `episodes_and_seasons_can_be_monitored_and_listed`, `editing_a_series_and_refreshing_it`
- Root folders, free space, path validation: `root_folders_must_exist`, `free_space_is_checked_last`, `removing_a_series_never_deletes_outside_the_root_folders`
- Library import: `an_existing_library_can_be_found_and_imported`, `existing_series_folders_can_be_adopted_with_their_files`
- Series types (standard, daily, anime): `anime_gets_absolute_numbers`, `daily_and_anime_formats`
- Scene numbering: `xem_numbers_are_stored_and_cleared_when_the_mapping_goes`, `a_release_with_scene_numbers_and_an_alternate_title_matches`
- Collections, minimum availability: `collections_show_what_is_owned_and_add_the_rest`, `movies_can_be_edited_and_removed_in_bulk`
- Tags: `delay_profiles_release_profiles_and_tags`
- Bulk edit and mass delete: `movies_can_be_edited_and_removed_in_bulk`, `the_series_editor_changes_several_series_at_once`, `several_series_can_be_removed_at_once`, and e2e "movies can be selected…", "series can be selected…"
- Manual and interactive import: `manual_import_lists_matches_and_imports_what_a_person_chose`
- Unmapped files; deletion; recycle bin: `folders_not_in_the_library_are_listed_with_their_titles`, `files_and_folders_are_deleted_safely_or_recycled`, `deleting_files_refuses_folders_outside_the_root_folders`
- Extras and metadata files: `subtitles_come_along_when_that_is_switched_on`, `metadata_files_are_written_only_when_asked_for`

## 4.2 Metadata
- Radarr metadata server with TMDB fallback: `a_radarr_server_movie_maps_to_our_metadata`, `tmdb_answers_when_the_radarr_server_cannot`, and the live test `the_real_server_knows_the_matrix` (run with `--ignored`)
- TV via SkyHook with TMDB fallback: see `crates/omp-metadata/tests/skyhook.rs`
- Refresh: `refreshing_keeps_choices_and_follows_the_provider`
- Image cache: `pictures_are_cached_and_only_from_allowed_hosts`
- Rate limit and cache: `repeated_lookups_hit_the_cache_and_a_429_is_retried`, `calls_are_spaced_out`

## 4.3 Quality and decisions
- Quality definitions: `quality_definitions_and_decision_settings`
- Profiles: `the_profile_decides_which_qualities_are_wanted`, `quality_profiles_round_trip`
- Custom formats: `custom_formats_are_validated_and_managed`, `custom_format_scores_come_from_the_profile`
- Release profiles: `release_profiles_filter_by_title_terms`
- Languages: `crates/omp-parser/tests/languages.rs`
- Delay profiles: `delay_profiles_hold_releases_back`, `delayed_releases_go_to_pending_once`
- Seeders minimum, seed goals: `an_indexers_seeding_goals_replace_the_clients`, `seeding_goal_keeps_files_in_place_until_met`
- Reasons for rejections: `unknown_and_unparsable_releases`, `choosing_a_release_reports_why_it_could_not_be_sent`
- Upgrades: `a_file_below_the_cutoff_can_be_upgraded`, `an_existing_file_at_the_cutoff_ends_the_search_for_upgrades`, `upgrades_replace_and_equal_quality_is_refused`
- Blocklist: `blocklisted_releases_are_rejected`, `blocklist_and_queue_removal_handle_missing_entries`

## 4.4 Search and acquisition
- Indexer management: `indexers_and_download_clients_are_managed_through_the_api`, `indexer_plugins_are_validated_tested_and_searched`
- RSS sync: `rss_sync_grabs_the_best_release_and_remembers_where_it_stopped`, `rss_sync_failure_backs_the_indexer_off_and_skips_it_next_time`
- Automatic search: `automatic_search_grabs_and_interactive_does_not`
- Interactive search and grab: `interactive_search_and_grab_validate_their_input`
- Season packs: `a_season_pack_covers_all_missing_episodes_with_one_grab`, `a_season_pack_imports_each_episode_with_renaming`
- Wanted: `wanted_lists_show_missing_movies_and_files_below_the_cutoff`
- Calendar and iCal: `the_calendar_can_be_subscribed_to_as_an_ical_feed`
- Import lists: `import_lists_add_movies_once_and_respect_exclusions`, `import_lists_add_series_named_by_tvdb_id`
- Release push: `pushed_releases_are_judged_like_any_other`

## 4.5 Download and import
- Queue and control: `queue_items_can_be_paused_resumed_and_prioritised`, `downloads_can_be_paused_prioritised_and_queued`
- Completed downloads: `finished_downloads_are_imported_and_removed_from_the_client`, `a_finished_download_is_renamed_moved_and_recorded`
- Failed downloads: `a_failed_download_is_recorded_blocklisted_and_removed`, `a_failed_download_is_recorded_and_blocklisted_per_series`
- Hardlink, copy, move, atomic copy: `hardlinking_shares_the_file_and_keeps_the_original`, `copying_without_hardlinks_makes_a_separate_file`, `moving_removes_the_original`, `an_atomic_copy_leaves_no_partial_file_and_fails_cleanly`
- Remote path mappings: `a_client_path_is_translated_before_importing`, `remote_path_mappings_are_validated_and_listed`
- Renaming and preview: `existing_files_can_be_previewed_and_renamed_to_the_naming_format`, `renaming_moves_files_to_the_naming_format`
- Folder naming, multi-episode styles: `multi_episode_styles`, `folders`, `title_tokens_follow_the_pattern_separator_and_case`
- Media info: `probes_a_real_file_and_survives_a_bad_one`
- Upgrade replaces the old file: `an_upgrade_replaces_the_old_file_and_a_downgrade_is_refused`
- Unmonitor on delete: `files_and_folders_are_deleted_safely_or_recycled`

## 4.6 System
- Authentication: `setup_creates_the_one_user_and_logs_them_in`, `login_logout_and_bad_credentials`, `the_api_key_works_in_a_header_or_the_query_and_can_be_replaced`, `repeated_wrong_passwords_lock_the_username_out_for_a_while`, `the_local_network_can_skip_signing_in_only_when_allowed`
- Notifications: `notifications_validate_test_and_follow_history`, `email_notifications_are_checked_and_sent_over_smtp`, `crates/omp-plugin-host/tests/notifiers.rs`
- Scheduled tasks: `scheduled_tasks_are_listed_with_their_schedule`
- History: `wanted_calendar_history_and_blocklist_cover_episodes`, `history_is_newest_first_and_joins_by_download_id`
- Event log and retention: `events_reach_the_log_with_noise_filtered_out`, `host_settings_are_checked_saved_and_never_leak_the_proxy_password`
- Backup and restore: `backups_can_be_made_listed_downloaded_and_deleted`, `a_backup_can_be_staged_for_restore`, `a_staged_backup_replaces_the_database_on_the_next_open`; plugin sources are kept in the database: `a_plugin_is_previewed_installed_and_removed_from_a_url`
- Health: `health_checks_report_what_is_missing_and_clear_as_it_is_fixed`
- Disk space and status: `health_status_and_ui_fallback`, `free_space_is_checked_last`, the page tests in `e2e/smoke.spec.ts`
- Global settings (port, URL base, HTTPS, proxy): `host_settings_are_checked_saved_and_never_leak_the_proxy_password`, `saved_settings_apply_at_start_unless_the_environment_decides`, `the_app_can_be_served_over_https`, `requests_go_through_the_proxy_except_for_bypassed_hosts`, `behind_a_prefix_everything_moves_under_it`
- UI preferences, language scaffold: `ui_preferences_round_trip_and_reject_unknown_choices`, e2e "the dark theme can be chosen"
- REST API, OpenAPI, WebSocket: `every_described_route_exists_and_is_protected`, `a_change_through_the_api_reaches_connected_clients`, `the_socket_needs_authentication_and_a_same_origin_page`

## 4.7 Built-in downloaders
- Torrent: `adds_from_a_url_tracks_to_completion_and_removes`, `bad_sources_are_reported`
- Usenet: unit tests in `crates/omp-usenet/src` (NZB, yEnc, NNTP, limiter, post-processing, pause and priority)
- Both built-in downloaders are there from the first start and switched on; a library with its own client is left alone: `a_new_library_has_both_built_in_downloaders_switched_on`, `a_library_with_its_own_download_client_is_left_alone`
- External clients as plugins: the four tests in `crates/omp-plugin-host/tests/clients.rs`

## Torrent sites and usenet indexers (Prowlarr definitions)
- The built-in usenet downloader with no news server is not reported as a failure; it becomes a setup warning once a usenet indexer is enabled: `built_in_usenet_without_a_news_server_is_not_an_error_until_an_indexer_needs_it`
- Every bundled definition is read, every selector and filter in them is understood, and each builds a search request: `every_bundled_definition_is_read`, `the_selectors_of_html_definitions_parse`, `every_filter_a_definition_uses_exists`, `every_definition_builds_a_search_request`
- The selector engine, templates, filters, JSON paths, dates, numbers and categories: `tags_classes_attributes_and_combinators`, `positions`, `contains_not_and_has`, `plain_text_and_variables`, `if_else_on_whether_a_value_is_set`, `and_or_eq_ne`, `re_replace_join_and_range`, `text_filters`, `address_and_date_filters`, `paths_reach_values`, `row_filters_pick_rows`, `dates_by_layout`, `dates_of_unknown_shape`, `dotnet_patterns_are_made_readable`, `a_site_s_categories_map_both_ways`
- A public HTML site, a JSON API and an RSS feed are searched and read, with settings choosing what is sent, failures named and Cloudflare recognised: `a_public_html_site_is_searched_and_read`, `settings_choose_what_is_sent`, `a_json_site_is_read`, `an_rss_feed_is_read`, `a_failing_site_is_an_error_and_a_blocked_one_says_why`
- Private sites: the sign-in form with its hidden fields, a wrong password in the site's words, the torrent behind a details page, a pasted cookie: `a_private_site_is_signed_into_with_the_form_it_shows`, `a_wrong_password_is_reported_with_the_sites_words`, `the_torrent_behind_a_details_page_is_found_after_signing_in`, `testing_checks_the_sign_in_and_a_search`, `a_pasted_cookie_is_the_session`
- Through the app: the list of sites and usenet indexers, adding, testing, an interactive search, TV searches by episode number, and the app fetching a torrent for a client: `the_catalogue_lists_torrent_sites_and_usenet_indexers`, `a_site_is_tested_saved_searched_and_its_torrent_fetched`, `tv_searches_ask_the_site_for_the_episode_by_its_number`, `a_site_link_round_trips`, `the_bundled_library_lists_its_sites`, `a_folder_of_definitions_adds_and_replaces`
- A site behind Cloudflare's check is named as such, and read once FlareSolverr is set up (its address checked first): `a_site_behind_cloudflare_is_read_through_flaresolverr`
- The same engine as a plugin, run in the sandbox: `a_public_site_is_searched_from_inside_the_sandbox`
- The Indexers tab with its two lists opens: e2e in `e2e/smoke.spec.ts` ("settings tabs open", "indexers can be found by filtering")

## Plex and Jellyfin
- Reading a Jellyfin or Emby library page by page, ids, episodes with their series, the key kept out of the URL: `jellyfin_reads_its_whole_library_page_by_page`, `jellyfin_tests_the_connection_and_explains_failures`
- Reading a Plex library section by section (movies, then shows, then episodes, music skipped): `plex_walks_movie_sections_then_shows_then_episodes`, `plex_tests_the_connection_and_explains_failures`
- Both plugins describe their settings with the key hidden: `both_plugins_describe_their_settings`
- Through the API: a server is tested, saved, read by a sync; movies show which server holds them; status counts; what is on the server and not in the library; a failing server keeps its last good copy and says why; disabling and removing: `a_jellyfin_library_shows_what_is_already_there`
- Matching by TMDB id, IMDb id, then title and year; a series by id, or by title only when the server gave no ids; episodes by season and number: `a_movie_is_found_by_tmdb_then_imdb_then_title_and_year`, `a_title_only_matches_when_the_server_gave_no_ids`, `episodes_name_the_servers_that_have_them`, `annotating_adds_servers_and_episode_counts`
- The migration that adds the media server kind rebuilds the providers table without losing the rows that refer to it: `existing_providers_and_what_refers_to_them_survive_the_rebuild`
- Wanted → Missing leaves out movies and episodes a media server already holds, and lists them again when the server is switched off: `a_jellyfin_library_shows_what_is_already_there`
- The Media servers settings tab opens: e2e in `e2e/smoke.spec.ts` ("settings tabs open")

- The VPN is set up from Settings, saved for the entrypoint with owner-only access, secrets never sent back, a blank secret keeps the saved one, the environment locks the page: `the_vpn_is_set_up_from_the_page_and_its_secrets_stay_in_the_file`, `settings_are_saved_for_the_entrypoint_and_read_back`, `a_blank_secret_keeps_the_saved_one_and_no_provider_turns_the_vpn_off`, `nonsense_is_refused`

- Real public sites on the internet (The Pirate Bay, LimeTorrents, TorrentDownloads, LinuxTracker, Nyaa), run by hand: `crates/omp-cardigann/tests/live.rs`. They returned results with sizes, seeders and magnet links. EZTV, 1337x and Tokyo Tosho answered with Cloudflare's check and said so. No private site was tried, because that needs an account.

## Run against the real programs (Docker, on request)
These are ignored by default because they need Docker and the internet. All were run and passed for 1.0.
- qBittorrent, Transmission, SABnzbd and NZBGet (linuxserver images): `crates/omp-plugin-host/tests/real_clients.rs`. Each runs test connection, add, list, pause, resume, remove. They found and fixed two bugs the scripted servers had hidden: current qBittorrent answers 204 with a `QBT_SID_<port>` cookie (and 401 for a bad login), and Transmission reports a magnet link with no metadata as having nothing left to download, which was being treated as finished.
- A real Jellyfin (linuxserver image): `a_real_jellyfin_library_is_read_with_its_ids` skips the wizard, scans a movie and a show named with `[tmdbid-…]`, and reads them back through the plugin with their ids and episode numbers. It found that current Jellyfin needs the key in an Authorization header, not only X-Emby-Token. Plex needs an account to claim a server, so it has no run like this one.
- NNTP, yEnc and the downloader against INN 2.7: `crates/omp-usenet/tests/real_server.rs` posts a three-part file, downloads it, and checks every byte, plus a missing article is reported rather than hanging.
- A commercial usenet provider (Newshosting, TLS on 563): `crates/omp-usenet/tests/commercial_provider.rs` logs in, checks a wrong password is refused, downloads and decodes a live article, and checks a missing article is reported and the connection survives. Read-only, since the provider refuses posting. Needs `OMP_NNTP_HOST`, `OMP_NNTP_USER`, `OMP_NNTP_PASS` and `OMP_NNTP_ARTICLE`. Passed once for 1.0 (716,800 decoded bytes).
- The built-in VPN (Gluetun) in the real image, run by hand with docker run: no VPN setting starts the app as the unprivileged user as before; a VPN setting without the tunnel device stops with a clear message; with the Mullvad provider and a WireGuard key Gluetun brings the tunnel up, the app starts after it, the web port stays reachable, the VPN status endpoint reports running with the exit address, and docker stop ends both processes. Not run: a paid account for each provider. See docs/vpn.md.

## Not covered by an automated test
- Nothing is left that a person could not run. The tests marked ignored run on request.

## Browsing TMDB

- Discover opens on a trending title with a backdrop and a story (people and titles without one are passed over), marks what the library already has, and leaves out a chart that TMDB fails to serve: `discover_leads_with_a_title_that_has_a_backdrop_and_marks_what_is_owned`
- Search is paged, and a bad term or kind is refused: `search_is_paged`
- A title lists its cast, directors, writers, producers (one person on two jobs once, other crew left out), companies, trailer and recommendations: `a_title_carries_its_cast_crew_and_companies`
- A person's works are grouped by what they did, newest first, and flagged when in the library: `a_person_has_their_works_by_what_they_did_newest_first`
- A company's movies and series come a page at a time: `a_company_lists_its_movies_and_series_a_page_at_a_time`
- Without a TMDB key (only possible when a caller builds the app without one) the pages say what is needed: `browsing_says_what_it_needs_when_there_is_no_tmdb_key`
- TMDB works with no set-up: Radarr's public read-only token is built in. The environment's token wins, then one saved under Settings → Network, then the built-in one; a saved token is checked, never sent back, and can be reset: `tmdb_uses_the_built_in_token_unless_the_owner_brings_one`, `a_tmdb_token_is_checked_kept_out_of_replies_and_can_be_reset`
- Live: the Discover, title, person and company pages were driven in a browser against the real TMDB.
