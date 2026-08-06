# syntax=docker/dockerfile:1.7
# Multi-stage build for the Grass Worker Control API and Node.
#
# The runtime image contains both binaries; the default command starts the
# Control API. Start a Node with: docker run ... grass-node --config ...

# --- Console build -----------------------------------------------------------
FROM oven/bun:1.3 AS console-builder
WORKDIR /app
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl unzip ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && curl -fsSL https://vite.plus | bash
ENV PATH="/root/.vite-plus/bin:${PATH}"
COPY apps/console/package.json apps/console/bun.lock apps/console/
WORKDIR /app/apps/console
RUN vp install --frozen-lockfile
COPY apps/console/ /app/apps/console/
RUN vp build

# --- Rust build --------------------------------------------------------------
FROM rust:1.88-slim-bookworm AS rust-base
ARG TARGETARCH
WORKDIR /app
RUN apt-get update \
    && apt-get install -y --no-install-recommends binutils musl-tools pkg-config \
    && rm -rf /var/lib/apt/lists/* \
    && rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl \
    && cargo install cargo-chef --version 0.1.77 --locked

FROM rust-base AS rust-planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM rust-base AS rust-dependencies
ARG TARGETARCH
COPY --from=rust-planner /app/recipe.json recipe.json
# grass-assets' build script needs a directory during dependency cooking. The
# real Console output is copied into the final build stage below.
RUN case "$TARGETARCH" in \
      amd64) target=x86_64-unknown-linux-musl ;; \
      arm64) target=aarch64-unknown-linux-musl ;; \
      *) echo "Unsupported target architecture: $TARGETARCH" >&2; exit 1 ;; \
    esac \
    && mkdir -p apps/console/dist \
    && printf '<!doctype html><title>placeholder</title>\n' > apps/console/dist/index.html \
    && cargo chef cook --release --target "$target" \
        --recipe-path recipe.json

FROM rust-base AS rust-builder
ARG TARGETARCH
COPY --from=rust-dependencies /app/target /app/target
COPY Cargo.toml Cargo.lock ./
COPY apps/control-api/ apps/control-api/
COPY apps/node/ apps/node/
COPY crates/ crates/
COPY --from=console-builder /app/apps/console/dist/ apps/console/dist/
RUN case "$TARGETARCH" in \
      amd64) target=x86_64-unknown-linux-musl ;; \
      arm64) target=aarch64-unknown-linux-musl ;; \
      *) echo "Unsupported target architecture: $TARGETARCH" >&2; exit 1 ;; \
    esac \
    && cargo build --release --target "$target" \
        -p grass-control-api -p grass-node \
    && strip --strip-unneeded \
        "target/$target/release/grass-control-api" \
        "target/$target/release/grass-node" \
    && mkdir /app/out \
    && cp "target/$target/release/grass-control-api" /app/out/grass-control-api \
    && cp "target/$target/release/grass-node" /app/out/grass-node

# --- Git build ---------------------------------------------------------------
# Git 2.49 introduced http.curloptResolve, which the Node uses to pin HTTP(S)
# clones to an address that already passed the repository network policy.
FROM debian:bookworm-slim AS git-builder
ARG GIT_VERSION=2.49.1
ARG GIT_SHA256=310831de967f1c8c5e8ff55f92807dea89f83dc3d3d2a5d16c209bd01a31def1
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates curl gcc gettext libcurl4-openssl-dev libexpat1-dev \
        libssl-dev make perl xz-utils zlib1g-dev \
    && rm -rf /var/lib/apt/lists/* \
    && curl --fail --location --retry 3 \
        "https://www.kernel.org/pub/software/scm/git/git-${GIT_VERSION}.tar.xz" \
        --output /tmp/git.tar.xz \
    && echo "${GIT_SHA256}  /tmp/git.tar.xz" | sha256sum --check --strict - \
    && mkdir /tmp/git \
    && tar --extract --xz --file /tmp/git.tar.xz --directory /tmp/git --strip-components=1 \
    && make -C /tmp/git -j2 prefix=/usr/local NO_TCLTK=YesPlease NO_GETTEXT=YesPlease \
    && make -C /tmp/git prefix=/usr/local NO_TCLTK=YesPlease NO_GETTEXT=YesPlease install

# --- Runtime helpers ---------------------------------------------------------
FROM debian:bookworm AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates libcurl4 libexpat1 openssh-client perl zlib1g \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home --uid 10001 grass \
    && mkdir -p /data && chown grass:grass /data
COPY --from=git-builder /usr/local/ /usr/local/
COPY --from=rust-builder /app/out/grass-control-api /usr/local/bin/grass-control-api
COPY --from=rust-builder /app/out/grass-node /usr/local/bin/grass-node
USER grass
WORKDIR /home/grass
VOLUME ["/data"]
EXPOSE 7817 8080
CMD ["grass-control-api"]

FROM debian:bookworm-slim AS runtime-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates libcurl4 libexpat1 openssh-client perl zlib1g \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home --uid 10001 grass \
    && mkdir -p /data && chown grass:grass /data
COPY --from=git-builder /usr/local/ /usr/local/
COPY --from=rust-builder /app/out/grass-control-api /usr/local/bin/grass-control-api
COPY --from=rust-builder /app/out/grass-node /usr/local/bin/grass-node
USER grass
WORKDIR /home/grass
VOLUME ["/data"]
EXPOSE 7817 8080
CMD ["grass-control-api"]

FROM alpine:3.22 AS runtime-alpine
RUN apk add --no-cache ca-certificates git openssh-client \
    && addgroup -S -g 10001 grass \
    && adduser -S -D -u 10001 -G grass -h /home/grass grass \
    && mkdir -p /data \
    && chown grass:grass /data /home/grass
COPY --from=rust-builder /app/out/grass-control-api /usr/local/bin/grass-control-api
COPY --from=rust-builder /app/out/grass-node /usr/local/bin/grass-node
USER grass
WORKDIR /home/grass
VOLUME ["/data"]
EXPOSE 7817 8080
CMD ["grass-control-api"]

# Keep the unqualified `docker build` behavior aligned with the default Debian
# image while retaining explicit `runtime-alpine` and `runtime-slim` targets.
FROM runtime AS default
