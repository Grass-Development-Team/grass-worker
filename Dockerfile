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
FROM rust:1.88-slim-bookworm AS rust-builder
WORKDIR /app
RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config \
    && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY apps/control-api/ apps/control-api/
COPY apps/node/ apps/node/
COPY crates/ crates/
COPY --from=console-builder /app/apps/console/dist/ apps/console/dist/
RUN cargo build --release -p grass-control-api -p grass-node

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

# --- Runtime -----------------------------------------------------------------
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates libcurl4 libexpat1 openssh-client perl zlib1g \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home --uid 10001 grass \
    && mkdir -p /data && chown grass:grass /data
COPY --from=git-builder /usr/local/ /usr/local/
COPY --from=rust-builder /app/target/release/grass-control-api /usr/local/bin/grass-control-api
COPY --from=rust-builder /app/target/release/grass-node /usr/local/bin/grass-node
USER grass
WORKDIR /home/grass
VOLUME ["/data"]
EXPOSE 7817 8080
CMD ["grass-control-api"]
