# Grass Worker

> [!IMPORTANT]
> This is an AI-generated project currently undergoing human review. DO NOT USE IN A PRODUCTION ENVIRONMENT.

Grass Worker is a self-hosted deployment platform.

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
