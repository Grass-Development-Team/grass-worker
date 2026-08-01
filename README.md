# Grass Worker

Grass Worker is a self-hosted deployment platform: a Vercel-style
Deployments Page backed by a Control API, build-and-serve Nodes, teams,
quotas, automatic platform domains, realtime build logs, and release
reviews.

- **Control API** — HTTP API, setup flow, auth and sessions, teams and
  roles, quota enforcement, projects, deployments, host provisioning,
  reviews, audit, and the embedded Web Console.
- **Node** — claims deployments, builds them inside a Podman/Docker socket
  container runtime, normalizes output to Grass Output v1, uploads
  artifacts, and serves static sites on public hosts.
- **Console** — React + shadcn/ui interface for the whole flow, from setup
  to rollback.

## Quick start

See [docs/self-hosting.md](docs/self-hosting.md) for the full walkthrough.

```sh
just install console   # Console dependencies (Vite+)
just quality           # fmt + clippy + tests + checks + build
just run api           # Control API (setup mode on an empty database)
just run node          # Build/serve Node
just run console       # Console dev server
```

Current version and remaining work live in [docs/todo.md](docs/todo.md).

## License

Grass Worker is licensed under the [BSD 3-Clause License](LICENSE).
