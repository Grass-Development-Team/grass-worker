# Grass Worker default build image: node:22 plus bun and corepack shims so
# auto-detected bun/pnpm/yarn projects build out of the box.
#
#   docker build -t grass-build:local docker/build-image
#
# Point [runtime] default_build_image at the tag to use it.
FROM docker.io/library/node:22
RUN npm install -g bun
RUN corepack enable
