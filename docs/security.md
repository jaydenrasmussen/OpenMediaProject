# Security review

Reviewed for 1.0. Each line names the control and where a test shows it works.

## Who can do what

- Every API route except health, `auth/status`, `auth/setup` and `auth/login` needs a session cookie or the API key (`crates/omp-app/tests/auth.rs`, and the OpenAPI test checks that every described route answers 401 without credentials).
- Passwords are hashed with argon2. Failed logins are rate limited per client. The session cookie is `HttpOnly`, `SameSite`, and `Secure` behind an HTTPS proxy.
- The WebSocket checks that the request's origin matches the host (`crates/omp-app/tests/events.rs`).
- "Skip sign-in on the local network" is off by default, applies only to private addresses, never to requests carrying proxy headers, and only once an account exists.
- The setup endpoint works only while no account exists.

## Requests the server makes for a user (SSRF)

- **Artwork** (`/image`): fetched only from the hosts in the image allow list (TMDB and TVDB by default), redirects are refused, size and time are capped (`crates/omp-app/src/images.rs`).
- **Plugin install by URL**: http and https only, 32 MB cap, 60 s timeout. It can reach any address, like an indexer URL can; both need a signed-in user. The preview shows the package's SHA-256 and the hosts it may reach, and install must quote that hash.
- **Indexers, download clients, notifications:** the address is the user's choice by design (a client on the LAN is the normal case).
- **Plugin web requests** go through the host: http and https only, only to hosts the manifest lists. A manifest of `["*"]` allows any host, including the LAN, which download-client plugins need; the install preview states it.

## Files and paths

- UI files: a request path with `..` or a backslash is refused (`crates/omp-app/tests/ui_base.rs`).
- Plugin ids are checked before they become a folder name; zip entry names are read, never used as paths; one folder of files is written, from a staging folder, by rename.
- Backups are addressed by name through a lookup, not a joined path.
- Usenet file names from an NZB are reduced to a safe file name before writing (`safe_file_name`), and archive extraction runs in the download's own folder.
- Deleting files and folders is limited to paths under a configured root folder and can go to a recycle bin.

## Plugin sandbox

- No filesystem, no sockets, no environment: a plugin sees only the `host` interface.
- Each call gets a fresh instance. Limits per call: 500 million units of fuel (CPU), 256 MB of memory, 100 web requests, 16 MB per response. Tests: `runaway_cpu_is_stopped_by_fuel`, `memory_hogs_are_stopped`, `a_call_may_make_only_so_many_web_requests` in `crates/omp-plugin-host/tests/plugins.rs`.
- A panic or trap fails that call only (`panic_is_contained_and_host_keeps_working`).
- Errors returned to plugins and logs leave out request URLs, which can hold API keys.

## Secrets

- Provider settings (API keys, passwords) are stored in the database in clear text, as Radarr does; the data folder should be private. The proxy password is never returned by the API.
- The API key can be regenerated. Backups contain these settings and should be treated the same way.

## Known limits

- Plugins with `allowed_hosts = ["*"]` can reach internal services. Install such plugins only from sources you trust.
- There is one user role; any signed-in user can do everything.
- Wall-clock time of one plugin call is bounded by the per-request timeouts and the 100-request limit, not by a single overall timer.
