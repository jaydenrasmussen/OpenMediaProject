# syntax=docker/dockerfile:1

# ---- UI: static Next.js export -------------------------------------------
FROM node:22-slim AS ui
WORKDIR /ui
COPY ui/package.json ui/package-lock.json ./
RUN npm ci
COPY ui/ ./
RUN npm run build

# ---- Server ---------------------------------------------------------------
FROM rust:1-bookworm AS server
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates crates
COPY wit wit
# The UI is compiled into the binary.
COPY --from=ui /ui/out ui/out
# rust-toolchain.toml lists the wasm target; the server build does not need it.
RUN rustup target add wasm32-wasip2 || true
RUN cargo build --release -p omp-server

# ---- First-party plugins (WASM components; the same files run on any architecture) ----
# Built natively on the build machine, once, instead of per target architecture under emulation.
FROM --platform=$BUILDPLATFORM rust:1-bookworm AS plugins
WORKDIR /src
# One target directory for every plugin, so shared dependencies compile once.
ENV CARGO_TARGET_DIR=/cargo-target
RUN rustup target add wasm32-wasip2
# omp-cardigann takes its package settings from the workspace root.
COPY Cargo.toml ./
COPY wit wit
COPY plugins/webhook plugins/webhook
COPY plugins/custom-list plugins/custom-list
COPY plugins/discord plugins/discord
COPY plugins/slack plugins/slack
COPY plugins/telegram plugins/telegram
COPY plugins/ntfy plugins/ntfy
COPY plugins/gotify plugins/gotify
COPY plugins/pushover plugins/pushover
COPY plugins/plex plugins/plex
COPY plugins/jellyfin-emby plugins/jellyfin-emby
COPY plugins/sabnzbd plugins/sabnzbd
COPY plugins/qbittorrent plugins/qbittorrent
COPY plugins/transmission plugins/transmission
COPY plugins/nzbget plugins/nzbget
COPY plugins/plex-library plugins/plex-library
COPY plugins/jellyfin-library plugins/jellyfin-library
COPY crates/omp-cardigann crates/omp-cardigann
COPY plugins/prowlarr-indexers plugins/prowlarr-indexers
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    set -eux; \
    for p in webhook custom-list discord slack telegram ntfy gotify pushover plex jellyfin-emby sabnzbd qbittorrent transmission nzbget plex-library jellyfin-library prowlarr-indexers; do \
      (cd plugins/$p && cargo build --release --locked --target wasm32-wasip2); \
      mkdir -p /out/$p; \
      cp plugins/$p/plugin.toml /out/$p/; \
      cp /cargo-target/wasm32-wasip2/release/$(echo $p | tr - _).wasm /out/$p/; \
    done

# ---- Bundled tools: official 7-Zip (has the RAR codec; distro builds may not), par2, static ffprobe ----
FROM debian:bookworm-slim AS tools
ARG TARGETARCH
ARG SEVENZIP_VERSION=2604
ARG PAR2_VERSION=1.5.0
ARG FFMPEG_VERSION=7.0.2
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl xz-utils unzip \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /tools
RUN set -eux; \
    case "$TARGETARCH" in \
      amd64) SZ=x64;   SZ_SHA=fc0327ba27e89bd086cf426dff17d77de582953cdbbc10a6576540a06853ffcd; \
             P2=amd64; P2_SHA=5a9f64386813456693c2ea1fb7649436fe7544bbdf97fd73b3483dfcc8aca464; \
             FF_SHA=abda8d77ce8309141f83ab8edf0596834087c52467f6badf376a6a2a4c87cf67 ;; \
      arm64) SZ=arm64; SZ_SHA=5b0ac3aa91c1e3499f011d6af69d7f6b4de3ef2f011d9e2ff44c58ebd04aa388; \
             P2=arm64; P2_SHA=3afb2f0b319fc4e6353c6d4261994757cbe3189f968a72420b4ba1d63f9c9a70; \
             FF_SHA=f4149bb2b0784e30e99bdda85471c9b5930d3402014e934a5098b41d0f7201b1 ;; \
      *) echo "unsupported arch $TARGETARCH"; exit 1 ;; \
    esac; \
    curl -fsSLo 7z.tar.xz "https://www.7-zip.org/a/7z${SEVENZIP_VERSION}-linux-${SZ}.tar.xz"; \
    echo "${SZ_SHA}  7z.tar.xz" | sha256sum -c -; \
    tar -xf 7z.tar.xz 7zz License.txt; \
    curl -fsSLo par2.zip "https://github.com/animetosho/par2cmdline-turbo/releases/download/v${PAR2_VERSION}/par2cmdline-turbo-${PAR2_VERSION}-linux-${P2}.zip"; \
    echo "${P2_SHA}  par2.zip" | sha256sum -c -; \
    unzip -o par2.zip par2; \
    curl -fsSLo ff.tar.xz "https://johnvansickle.com/ffmpeg/releases/ffmpeg-${FFMPEG_VERSION}-${P2}-static.tar.xz"; \
    echo "${FF_SHA}  ff.tar.xz" | sha256sum -c -; \
    tar -xf ff.tar.xz --strip-components=1 --wildcards '*/ffprobe' '*/GPLv3.txt'; \
    chmod +x 7zz par2 ffprobe

# ---- Gluetun: the VPN client (one static Go binary; its server lists are inside it) ----
FROM qmcgaw/gluetun:v3.41.3 AS gluetun
# Gluetun expects the defaults its own image sets as environment variables; keep them as a file.
RUN env | grep -v -E '^(HOME|PATH|HOSTNAME|PWD|OLDPWD|SHLVL|TERM|_)=' > /gluetun.defaults

# ---- Release tarballs: just the binary and the plugins (build with --target export) ----
FROM scratch AS export
COPY --from=server /src/target/release/omp-server /omp-server
COPY --from=plugins /out /plugins

# ---- Runtime --------------------------------------------------------------
FROM debian:bookworm-slim
# openvpn and iptables are what Gluetun drives (it looks for openvpn2.5/openvpn2.6 by those names).
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates openvpn iptables \
    && rm -rf /var/lib/apt/lists/* \
    && ln -s /usr/sbin/openvpn /usr/sbin/openvpn2.6 && ln -s /usr/sbin/openvpn /usr/sbin/openvpn2.5 \
    && useradd --system --uid 1000 --create-home omp \
    && mkdir -p /config /downloads /media && chown omp /config /downloads /media
COPY --from=tools /tools/7zz /usr/local/bin/7zz
COPY --from=tools /tools/par2 /usr/local/bin/par2
COPY --from=tools /tools/ffprobe /usr/local/bin/ffprobe
COPY --from=tools /tools/License.txt /usr/share/doc/7zip/License.txt
COPY --from=server /src/target/release/omp-server /usr/local/bin/omp-server
COPY --from=plugins /out /opt/omp/plugins
COPY --from=gluetun /gluetun-entrypoint /usr/local/bin/gluetun-entrypoint
COPY --from=gluetun /gluetun.defaults /etc/gluetun.defaults
# Gluetun reads this at start-up and refuses to run without it (it only logs the version).
COPY --from=gluetun /etc/alpine-release /etc/alpine-release
COPY docker/entrypoint.sh /usr/local/bin/omp-entrypoint

ENV OMP_ADDR=0.0.0.0:8080 OMP_DATA_DIR=/config OMP_DOWNLOADS_DIR=/downloads OMP_BUNDLED_PLUGINS_DIR=/opt/omp/plugins
EXPOSE 8080
VOLUME ["/config", "/downloads", "/media"]
# Starts as root so Gluetun can set up the tunnel; the entrypoint runs the app as `omp` (uid 1000).
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s CMD ["/usr/local/bin/omp-server", "--healthcheck"]
ENTRYPOINT ["/usr/local/bin/omp-entrypoint"]
