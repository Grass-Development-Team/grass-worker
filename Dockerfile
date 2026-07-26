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

# --- Runtime -----------------------------------------------------------------
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates git \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home --uid 10001 grass \
    && mkdir -p /data && chown grass:grass /data
COPY --from=rust-builder /app/target/release/grass-control-api /usr/local/bin/grass-control-api
COPY --from=rust-builder /app/target/release/grass-node /usr/local/bin/grass-node
USER grass
WORKDIR /home/grass
VOLUME ["/data"]
EXPOSE 7817 8080
CMD ["grass-control-api"]
