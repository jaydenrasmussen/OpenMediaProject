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
# rust-toolchain.toml lists the wasm target; the server build does not need it.
RUN rustup target add wasm32-wasip2 || true
RUN cargo build --release -p omp-server

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

# ---- Runtime --------------------------------------------------------------
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 1000 --create-home omp \
    && mkdir -p /config /downloads /media && chown omp /config /downloads /media
COPY --from=tools /tools/7zz /usr/local/bin/7zz
COPY --from=tools /tools/par2 /usr/local/bin/par2
COPY --from=tools /tools/ffprobe /usr/local/bin/ffprobe
COPY --from=tools /tools/License.txt /usr/share/doc/7zip/License.txt
COPY --from=server /src/target/release/omp-server /usr/local/bin/omp-server
COPY --from=ui /ui/out /app/ui

ENV OMP_ADDR=0.0.0.0:8080 OMP_UI_DIR=/app/ui
EXPOSE 8080
VOLUME ["/config", "/downloads", "/media"]
USER omp
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s CMD ["/usr/local/bin/omp-server", "--healthcheck"]
ENTRYPOINT ["/usr/local/bin/omp-server"]
