# Self-hosting Grass Worker

This guide walks through the first-stage end-to-end flow: setup, login,
project creation, an automatic platform domain, a container build, realtime
logs, review, activation, and public access.

## Requirements

- PostgreSQL 15+
- Redis 7+ (recommended; an in-memory fallback exists for evaluation)
- A container engine socket: Podman (rootless works) or Docker
- Git 2.49+ plus OpenSSH client tools (`ssh` and `ssh-keyscan`) available to
  the Node process
- Rust 1.85+ and [Vite+](https://viteplus.dev/) when building from source

PostgreSQL and Redis belong to the Control API. Nodes do not connect to
either service directly; they use the authenticated Control API instead.

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

Audit events are retained for 90 days by default. Configure another number
of days, or use `0` for permanent retention:

```toml
[audit]
retention_days = 90
```

`GWAPI_AUDIT_RETENTION_DAYS` overrides this value at runtime. The Control API
records user-facing API reads and writes, failed logins, authorization
denials, request IDs, actors, source addresses, results, durations, and
redacted change values. High-volume Node heartbeats, build-log chunks, route
polling, artifact/static transfers, WebSocket messages, and frontend static
assets are intentionally not recorded per item. A WebSocket handshake is
still recorded once as a user-facing read.

After setup, platform administrators can inspect and update non-secret
Control API settings under **Administration → Settings**. Secret settings
remain write-only: the Console reports whether each value is configured but
never returns it. File and environment configuration still provide the
startup values for settings that cannot be changed before the API is running.

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
Administration → Nodes.

### Choose Node capabilities

Every Node must enable at least one capability. A Build-only Node claims and
runs builds but does not accept site traffic:

```toml
[node.capabilities]
build = true
serve = false
```

A Serve-only Node stages assigned artifacts and serves traffic but does not
claim builds:

```toml
[node.capabilities]
build = false
serve = true
```

A combined Node supports both roles:

```toml
[node.capabilities]
build = true
serve = true
```

For every Serve-capable Node, set `serve.public_base_url` to an absolute
HTTP(S) URL that every other Serve Node and the Control API can reach. This
address is used for one-hop gateway traffic and immediate route invalidation,
so `127.0.0.1` is only suitable when the Control API and Node share the same
machine:

```toml
[serve]
public_base_url = "http://serve-a.internal:8080"
```

### Configure Serve scheduling capacity

By default a Serve Node reports 80% of its logical CPU capacity, 75% of total
memory, 80% of the available space on the filesystem containing
`serve.artifact_cache_root`, and 10 deployment slots. These values initialize
the Node's scheduling capacity on its first Serve registration.

Set explicit initial values when automatic detection is not appropriate:

```toml
[serve.capacity]
cpu_millicores = 1600
memory_mb = 1536
disk_mb = 8192
max_deployments = 10
```

`0` for CPU, memory, or disk keeps automatic detection; deployment capacity
must be positive. Administrators can inspect usage and persist later capacity
changes under Administration → Nodes.

The same page exposes every non-secret Node setting as desired configuration.
After saving, the Node reports `Pending`, `Applying`, `Applied`, or `Failed`
until its effective revision matches the desired revision. Node tokens and
other secret values are never returned as part of desired configuration.

### Drain and delete a Node

Deleting a Node is asynchronous. After the confirmation prompt, a Serve Node
with assigned services requires an administrator to select another eligible
Serve Node. The source enters the deletion queue, artifacts are synchronized
to the target, and routes switch atomically only after every replacement is
Ready. A Serve Node without assigned services can enter the queue directly.

A Build-capable Node stops claiming new work while Draining and remains in the
queue until its existing builds reach a terminal state. The Console shows the
queued, migrating, draining, deleting, failed, and completed progress states.
A failed deletion keeps the source routes intact, displays the failure reason,
and can be retried after the underlying capacity, connectivity, or artifact
problem is fixed. The Node token stops authenticating after deletion completes.

Each Static deployment initially reserves 50 millicores, 64 MB memory, and
256 MB disk. Each SSR deployment reserves 200 millicores, 256 MB memory, and
512 MB disk. The artifact upload replaces the disk estimate with its actual
unpacked size. Automatic placement selects the eligible Node with the lowest
projected dominant resource usage and randomizes exact ties; operators may
instead choose a specific Serve Node in the deployment dialog.

When every Node is at normal capacity, the scheduler may place up to two
additional deployments on each Serve Node. CPU, memory, or deployment slots
may be overcommitted in this mode, but disk is never overcommitted. A
deployment keeps one assigned Serve Node while it is part of the effective
delivery set. Superseded Preview deployments are retired after their
replacement reaches Serve Ready and no longer consume scheduler capacity.
Node-local cached files and Control API artifacts are retained until the
separate artifact-retention policy removes them.

## 4. Configure a host source

Public URLs need a host source. Point a wildcard DNS record
(`*.apps.example.com`) at any Serve Node listener (port 8080 by default),
then add a **Wildcard** host source with that base domain under
Administration → Host sources and mark it as the default.

Every Serve Node holds the same Host route snapshot. If DNS sends a request to
a Node that does not own that deployment, the receiving Node streams it to the
assigned Node through one authenticated peer hop. Make every
`serve.public_base_url` reachable from every Serve Node and the Control API,
and allow the serve port through internal firewalls.

Without wildcard DNS, add a **DNS provider (Cloudflare)** source instead:
provide an API token with the Zone / DNS / Edit permission, the zone ID,
and the record the platform should create for every domain (type `A`,
`AAAA`, or `CNAME` plus the node address as the value). Each provisioned
domain then becomes one DNS record created through the Cloudflare API;
bindings turn `failed` with the provider message when the API rejects a
request and can be retried from the project's Domains page. Credentials
are write-only: the API returns configured key names, never values, and
editing a source only overwrites the fields you fill in.

A **Manual** source assigns domains without touching DNS; bindings stay
`pending` until an operator creates the record and re-runs provisioning.

New projects automatically receive `slug.apps.example.com`; preview
deployments receive unique `slug-xxxxxxxx.apps.example.com` hosts.

## 5. Configure Git source access

Public repositories can use HTTP, HTTPS, SSH, scp-like SSH, or `git://` URLs.
HTTP and `git://` are always anonymous. A custom SSH port uses an `ssh://`
URL; scp-like syntax uses port 22.

Private credentials require an independent 32-byte master key. Generate a
base64url value and provide it to the Control API without committing it:

```sh
openssl rand -base64 32 | tr '+/' '-_' | tr -d '='
export GWAPI_GIT_CREDENTIAL_KEY_ID=primary
export GWAPI_GIT_CREDENTIAL_MASTER_KEY='<generated value>'
```

Team owners and admins can then add HTTPS username/token credentials or SSH
private keys under Team settings → Git credentials and bind a compatible
credential under Project → Settings → Build & Deployment. Credentials are
scoped to scheme, normalized host, and effective port; secrets are encrypted,
write-only, and never placed in repository URLs. Normal rotation keeps queued
deployments on their fixed version, while revocation invalidates old versions.

The first SSH checkout reports a fingerprint and stops. Verify it out of band,
then approve it under Team settings → SSH host keys. A changed key blocks future
checkouts until the new fingerprint is explicitly approved.

Nodes reject loopback, link-local, private, documentation, multicast, and other
non-public targets after resolving every address. A Node administrator may add
an exact exception in `node.toml`; all three fields must match:

```toml
[[security.private_repository_targets]]
host = "git.internal.example"
ip = "10.0.0.8"
port = 2222
```

CIDRs, wildcard hosts, and host-only exceptions are not supported. HTTP(S),
SSH, and `git://` connections are pinned to an address that passed this policy.

## 6. Deploy

1. Create a project with a Git repository URL and build settings; bind a team
   credential first when the repository is private.
2. Open the project → Deployments → **Deploy preview** or **Deploy
   production**.
3. Watch the realtime build log. A Build Node runs the build inside its
   configured container runtime, produces Grass Output v1, and streams the
   artifact to Control API local storage.
4. When the build becomes Ready, the Control API automatically creates any
   review required by policy. The assigned Serve Node then downloads,
   verifies, and stages the artifact. Platform administrators can inspect the
   pending review immediately, but Approve and Reject remain disabled until
   Serve is Ready.
5. Preview deployments activate automatically (default policy) and become
   reachable at their preview URL after Serve staging is ready.
6. Production deployments require review by default. Only platform
   administrators decide reviews under **Administration → Reviews**; team
   owners and administrators can inspect review state but cannot approve or
   reject it. **Approve & promote** publishes once the target is Serve Ready;
   plain **Approve** lets a team administrator Promote later. The review
   policy is configured under **Administration → Settings → Release review**
   (production defaults to manual, preview to auto).
7. Delivery uses a rolling overlap. While a replacement is Pending, Syncing,
   or Failed, the previous Ready Preview and current Active Production remain
   assigned and keep serving. Routes switch only after the replacement is
   Serve Ready; then superseded Preview assignments are retired. A cluster
   without enough temporary capacity rejects the new placement instead of
   stopping the old version.
8. Promoting or rolling back to a retired Production deployment first queues
   it for Serve synchronization. The current Production domain remains on the
   active version until the target reports Ready, then activation and route
   cutover happen atomically.

## Notes

- The Control API is the artifact relay between Build and Serve Nodes: Build
  Nodes upload to its local storage and Serve Nodes download into their local
  artifact cache. Size the Control API storage for retained artifacts as well
  as logs and other platform data.
- Every deployment receives a protected Preview host. Only active users in
  the project's current team and active platform administrators can open it.
  Project transfer, user disablement, or Preview replacement invalidates
  existing Preview grants on their next verification. HTTPS Preview callbacks
  use a Secure `__Host-` cookie. Plain HTTP remains available for trusted local
  development with a host-only cookie, but must not be exposed publicly.
- After upgrading an existing cluster to this version, restart every Node at
  least once so it registers its exact capabilities and Serve capacity.
- This phase runs exactly one Control API and assigns each deployment to one
  Serve Node. It does not provide Control API high availability, Build
  failover, automatic failover after an unexpected Serve outage, or object
  storage. Planned Serve Node deletion uses the drain-and-migrate workflow
  above; an unexpected outage still makes assigned sites unavailable until
  that Node returns or an administrator can complete a safe migration.
- Static outputs from Vite/React/Vue/Svelte SPAs, Next.js static export,
  Nuxt SPA/prerender, SvelteKit adapter-static, and Astro static are
  supported. **SSR deployments work for Next.js, Astro, and Nuxt**: the
  build produces a `.grass/output/server` bundle (Next.js standalone —
  requested automatically when no static export is configured; Astro needs
  the `@astrojs/node` adapter in standalone mode; Nuxt uses its Nitro
  output) and the node runs it in a service container on first request,
  reverse-proxying the domain to it. Services stop after 30 idle minutes
  (`[serve.ssr] idle_stop_seconds`) and restart on demand; the service
  image defaults to `node:22` (`[runtime] default_serve_image`).
  Containerized nodes reach SSR containers by container IP, so both must
  share a network — set `[runtime] network` (or `GWNODE_RUNTIME_NETWORK`)
  to the compose network when the node itself runs in a container. SSR
  service containers carry their owning Node label, so multiple Nodes may
  safely share one Docker or Podman socket.
  Hybrid, serverless, and edge outputs still fail with an explicit
  "not implemented yet" message. SSR project env vars are not implemented
  yet; SSR previews and reviews behave exactly like static ones.
- Static serving resolves the requested path directly. A missing path serves
  the deployment's `404.html` with status 404 when present, otherwise the
  platform default 404. SPA fallback is opt-in by providing `200.html`; an
  `index.html` alone is never used as a catch-all. HEAD and byte Range
  requests use the same streaming file implementation.
- Quota plans ship seeded (`free`, `student`, `plus`, `pro`, `ultra`);
  team groups map teams to plans, and usage appears under Usage.
- **Administration → Projects** lists every project on the platform with
  its team and latest deployment; administrators can archive or soft-delete
  projects from there.
- User-facing API access and key business actions are recorded with separate
  platform/team visibility; audit metadata and before/after values are
  redacted before storage.
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
