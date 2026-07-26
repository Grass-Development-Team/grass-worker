# Self-hosting Grass Worker

This guide walks through the first-stage end-to-end flow: setup, login,
project creation, an automatic platform domain, a container build, realtime
logs, review, activation, and public access.

## Requirements

- PostgreSQL 15+
- Redis 7+ (recommended; an in-memory fallback exists for evaluation)
- A container engine socket: Podman (rootless works) or Docker
- `git` available to the Node process
- Rust 1.85+ and [Vite+](https://viteplus.dev/) when building from source

## 1. Build

```sh
just install console
just build            # builds the Console, embeds it, and builds both binaries
```

Or use the Docker image (both binaries are included):

```sh
docker build -t grass-worker .
```

## 2. Start the Control API

Copy `config.toml.example` to `config.toml` and set at least the database
URL and Redis URL. Then:

```sh
just run api          # or: grass-control-api --config config.toml
```

With an empty database the service starts in **setup mode**. Open the
Console (`just run console` during development, or the embedded Console on
the Control API port in release builds) and finish the setup flow:

1. Database check
2. Initial administrator (becomes the platform `admin` with a personal team)
3. Site configuration
4. First Node — this generates the Node token; copy it now, it is shown once
5. Storage root (defaults to `/data`)
6. Finish — the service switches to ready mode

## 3. Start a Node

### Managed local Node (recommended for single-machine setups)

When the Control API and the Node run on the same machine, the Control API
can manage the Node process itself. Enable it in `config.toml`:

```toml
[node_manager]
auto_start_local_node = true
local_node_binary = "grass-node"
local_node_config = "./node.toml"
restart_on_exit = true
```

With this enabled:

- the setup wizard's Node step (or creating a node under Administration →
  Nodes with **Start local process** checked) generates
  `local_node_config` with the node token, a detected container runtime
  socket, and work directories under `{storage.root}/node`;
- the process starts automatically when setup finishes and on every
  Control API boot, restarts with backoff on unexpected exits, and stops
  with the Control API;
- Administration → Nodes shows the process state and offers
  start/stop/restart.

The generated file is written with mode 0600 because it contains the node
token; it is rewritten automatically if the storage root changes.
Hand-written node configs are never touched.

### Standalone Node

Copy `node.toml.example` to `node.toml`, paste the Node token, point
`control_api` at the Control API, and configure the container runtime
socket:

```toml
[runtime]
backend = "podman-socket"   # or "docker-socket"
socket = "unix:///run/user/1000/podman/podman.sock"
default_build_image = "docker.io/library/node:22"
```

```sh
just run node         # or: grass-node --config node.toml
```

The Node registers, heartbeats every 30 seconds, and appears under
Administration → Nodes. First-stage Nodes always build **and** serve; if the
config disables either capability it is corrected with a warning.

## 4. Configure a host source

Public URLs need a host source. Point a wildcard DNS record
(`*.apps.example.com`) at the Node serve listener (port 8080 by default),
then add a **Wildcard** host source with that base domain under
Administration → Host sources and mark it as the default.

New projects automatically receive `slug.apps.example.com`; preview
deployments receive unique `slug-xxxxxxxx.apps.example.com` hosts.

## 5. Deploy

1. Create a project with a public Git repository URL and build settings.
2. Open the project → Deployments → **Deploy preview** or **Deploy
   production**.
3. Watch the realtime build log. Builds run inside the configured container
   runtime, produce Grass Output v1, and upload the artifact.
4. Preview deployments activate automatically (default policy) and are
   reachable at their preview URL as soon as the build is ready.
5. Production deployments require review by default: request review, then
   approve as a team admin on the deployment page — or platform-wide under
   **Administration → Reviews**, where administrators see every pending
   review and can **Approve**, **Approve & promote** (publish in the same
   step), or **Reject** with a reason. After a plain approve, **Promote**
   publishes. The stable domain now serves the deployment. **Roll back**
   from any previously active deployment. The review requirement per
   environment is configured under **Administration → Settings → Release
   review** (production defaults to manual, preview to auto).

## Notes

- SSR, hybrid, serverless, and edge outputs fail with an explicit
  "not implemented yet" message in the first stage; static outputs from
  Vite/React/Vue/Svelte SPAs, Next.js static export, Nuxt SPA/prerender,
  SvelteKit adapter-static, and Astro static are supported.
- Quota plans ship seeded (`free`, `student`, `plus`, `pro`, `ultra`);
  team groups map teams to plans, and usage appears under Usage.
- **Administration → Projects** lists every project on the platform with
  its team and latest deployment; administrators can archive or soft-delete
  projects from there.
- Every key action is recorded under team and administrator audit pages.
- Secrets never land in the repository: node tokens are stored hashed, and
  DNS provider credentials belong in host source config or environment
  variables.

## Docker quick reference

Build `grass-build` first — the default build image with node, bun, and
corepack shims so auto-detected projects build regardless of package
manager:

```sh
docker build -t grass-build:local docker/build-image
```

Set `[runtime] default_build_image = "grass-build:local"` in the node
config to use it (the stock `docker.io/library/node:22` works too, but
projects whose scripts call bun will fail there).

The image contains both binaries, and the managed local Node makes a
single container the simplest deployment: enable
`[node_manager] auto_start_local_node` with
`local_node_config = "/data/node.toml"`, mount the engine socket, and the
Control API generates the node config during setup and runs `grass-node`
inside the same container. Builds copy the workspace through the engine
API (no host paths), so a socket from the host works as-is.

```sh
docker run -d --name grass-worker \
  -p 7817:7817 -p 8080:8080 \
  -v grass-data:/data \
  -v $PWD/config-dir:/home/grass/config \
  -v /var/run/docker.sock:/var/run/docker.sock \
  grass-worker grass-control-api --config /home/grass/config/config.toml
```

Separate containers still work for split deployments — run a second
container with `grass-node --config …` and a token created under
Administration → Nodes:

```sh
docker run -d --name grass-node \
  -p 8080:8080 -v grass-node-data:/data \
  -v /var/run/docker.sock:/var/run/docker.sock \
  -v $PWD/node.toml:/home/grass/node.toml:ro \
  grass-worker grass-node --config /home/grass/node.toml
```
