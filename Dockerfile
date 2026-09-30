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
FROM debian:bookworm-slim
# curl is for the health check. The optional video downloader needs yt-dlp
# (and ffmpeg for most sites); add them in an image of your own if you
# want it: FROM thencloud, then install both.
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
USER thencloud
VOLUME /data
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s CMD curl -fsS http://127.0.0.1:8080/api/health || exit 1
ENTRYPOINT ["thencloud-server"]
