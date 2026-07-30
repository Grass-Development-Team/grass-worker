# syntax=docker/dockerfile:1.7

# Grass Worker default build image: node:22-alpine plus bun and corepack shims so
# auto-detected bun/pnpm/yarn projects build out of the box.
#
#   docker build -t grass-build:local docker/build-image
#
# Point [runtime] default_build_image at the tag to use it.
FROM docker.io/library/node:22-alpine

ARG BUN_VERSION=1.3.14

RUN --mount=type=cache,id=grass-build-npm,target=/root/.npm \
    npm install --global --no-audit --no-fund --no-update-notifier "bun@${BUN_VERSION}" \
    && rm -rf /usr/local/lib/node_modules/bun/node_modules/@oven \
    && corepack enable
