# syntax=docker/dockerfile:1
#
# thencloud in a container: the server plus the web client it serves.
#
#   docker build -t thencloud .
#   docker run -p 8080:8080 -v thencloud-data:/data thencloud
#
# The web client is built the way scripts/release-web.sh builds it (same
# Rust toolchain, lockfiles and path mapping), so `thencloud verify-web`
# can check a server running this image against a signed release.
# See deploy/compose.yaml for running it behind a reverse proxy with HTTPS.

ARG RUST_VERSION=1.94.1

# --- web client: Rust crypto -> WASM, then Svelte + Tailwind via Vite -------
FROM node:24-bookworm AS web
ARG RUST_VERSION
# The release this is (a tag, as release-web.sh gets it): it's in every page,
# so a server running this image passes verify-web against that release.
ARG THENCLOUD_VERSION=dev
ENV CARGO_HOME=/cargo RUSTUP_HOME=/rustup PATH=/cargo/bin:$PATH
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal \
      --default-toolchain "$RUST_VERSION" --target wasm32-unknown-unknown \
 && cargo install wasm-pack --version 0.15.0 --locked
WORKDIR /thencloud
COPY Cargo.toml Cargo.lock ./
COPY .cargo .cargo
COPY crates crates
COPY web web
# As in release-web.sh: RUSTFLAGS replaces .cargo/config.toml's, so it
# repeats the getrandom cfg. The source and cargo home already sit at the
# paths release-web.sh maps them to, which the remaps spell out.
RUN RUSTFLAGS="--cfg getrandom_backend=\"wasm_js\" --remap-path-prefix=/thencloud=/thencloud --remap-path-prefix=/cargo=/cargo" \
      wasm-pack build crates/thencloud-wasm --release --target web --out-dir ../../web/src/wasm --no-typescript --no-pack \
 && cd web && npm ci --no-audit --no-fund && THENCLOUD_VERSION="$THENCLOUD_VERSION" npm run build

# --- server ---------------------------------------------------------------
FROM rust:${RUST_VERSION}-bookworm AS server
WORKDIR /thencloud
COPY Cargo.toml Cargo.lock ./
COPY .cargo .cargo
COPY crates crates
RUN cargo build --release --locked -p thencloud-server

# --- runtime --------------------------------------------------------------
FROM debian:bookworm-slim AS base
# curl is for the health check.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home-dir /data thencloud \
 && mkdir /data && chown thencloud /data
COPY --from=server /thencloud/target/release/thencloud-server /usr/local/bin/thencloud-server
COPY --from=web /thencloud/web/dist /usr/share/thencloud/web
ENV THENCLOUD_BIND=0.0.0.0:8080 \
    THENCLOUD_DATA_DIR=/data \
    THENCLOUD_WEB_DIR=/usr/share/thencloud/web
VOLUME /data
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s CMD curl -fsS http://127.0.0.1:8080/api/health || exit 1
ENTRYPOINT ["thencloud-server"]

# --- with the video downloader (optional) ---------------------------------
#   docker build --target with-downloader -t thencloud .
# or THENCLOUD_TARGET=with-downloader with deploy/compose.yaml. It stays off
# until an admin turns it on. yt-dlp (the zipapp, on Debian's python3),
# ffmpeg to merge video and audio, and deno, which yt-dlp needs for YouTube.
# Sites change often: to update yt-dlp, pass a newer YT_DLP_VERSION with its
# YT_DLP_SHA256 (the "yt-dlp" line of the release's SHA2-256SUMS).
FROM base AS with-downloader
ARG TARGETARCH
ARG YT_DLP_VERSION=2026.08.19
ARG YT_DLP_SHA256=1fa6733c37ea6fb51c99ad8fe785e7b7e5f3246c9b980230329d4fb72ed8d4d6
ARG DENO_VERSION=v2.9.7
ARG DENO_SHA256_AMD64=c6527f24f4b16031d3ae4fa9f658d5f11534c8d84ce7dc8502420280919c3490
ARG DENO_SHA256_ARM64=c832298b1ad4422481334855f6003e0f54145762c5a134f20a489511d2f65bbf
RUN apt-get update \
 && apt-get install -y --no-install-recommends ffmpeg python3 \
 && rm -rf /var/lib/apt/lists/* \
 && curl -fsSLo /usr/local/bin/yt-dlp \
      "https://github.com/yt-dlp/yt-dlp/releases/download/${YT_DLP_VERSION}/yt-dlp" \
 && echo "${YT_DLP_SHA256}  /usr/local/bin/yt-dlp" | sha256sum -c - \
 && chmod 755 /usr/local/bin/yt-dlp \
 && case "${TARGETARCH:-amd64}" in \
      amd64) arch=x86_64; sum="$DENO_SHA256_AMD64" ;; \
      arm64) arch=aarch64; sum="$DENO_SHA256_ARM64" ;; \
      *) echo "no deno for $TARGETARCH" >&2; exit 1 ;; \
    esac \
 && curl -fsSLo /tmp/deno.zip \
      "https://github.com/denoland/deno/releases/download/${DENO_VERSION}/deno-${arch}-unknown-linux-gnu.zip" \
 && echo "${sum}  /tmp/deno.zip" | sha256sum -c - \
 && python3 -m zipfile -e /tmp/deno.zip /usr/local/bin/ \
 && chmod 755 /usr/local/bin/deno \
 && rm /tmp/deno.zip
USER thencloud

# --- the default image: no downloader --------------------------------------
FROM base AS runtime
USER thencloud
