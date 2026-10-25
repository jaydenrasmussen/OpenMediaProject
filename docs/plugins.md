# Writing a plugin

A plugin is a WebAssembly component built against `wit/omp-plugin.wit`. There are four kinds, each a world in that file:

| Kind | World | You implement |
| --- | --- | --- |
| Indexer | `indexer-plugin` | `settings-schema`, `search`, `recent`, `test` |
| Notification | `notification-plugin` | `settings-schema`, `send` |
| Import list | `list-plugin` | `settings-schema`, `fetch` |
| Download client | `download-client-plugin` | `settings-schema`, `add`, `items`, `remove`, `pause`, `resume`, `test` |
| Media server | `media-server-plugin` | `settings-schema`, `library-page`, `test` |

## Package

A folder (or a zip with the same files) holding `plugin.toml` and the `.wasm`:

```toml
id = "my-plugin"            # lower case letters, digits and dashes
name = "My plugin"
version = "0.1.0"
kind = "notification"       # indexer | notification | list | download_client | media_server
protocol = "torrent"        # download_client only: torrent | usenet
wasm = "my_plugin.wasm"
description = "What it does"
allowed_hosts = ["api.example.com"]   # ["*"] allows any host
```

## What a plugin can do

- Call `host.http` / `host.http-get` (HTTP and HTTPS only) to hosts in `allowed_hosts`. The host adds the proxy, user agent and timeouts.
- Call `host.log`.
- Nothing else: no files, no sockets, no clock beyond WASI's basics. Every call starts a fresh instance with a fuel limit, so a plugin that loops is stopped.

Settings are described as JSON Schema by `settings-schema`; the UI builds the form from it and passes the saved values back as `settings-json`.

## Build

```
rustup target add wasm32-wasip2
cd plugins/my-plugin && cargo build --release --target wasm32-wasip2
```

`plugins/discord` (notification) and `plugins/qbittorrent` (download client) are short, complete examples. Tests in `crates/omp-plugin-host/tests` show how to run a plugin against a scripted `Fetcher`.

## Install

Settings, Plugins: give the address of a zip. The app downloads it, shows the hosts it may reach and a SHA-256, and installs only after you confirm that hash. Installed plugins are remembered in the database, so a restored backup can offer to fetch them again. Plugins installed this way live in the last folder of the plugin path; a plugin with the same id there replaces a bundled one.
