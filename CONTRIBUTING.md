# Contributing to Grass Worker

This guide applies to human and agent contributions. Grass Worker is under active
review and is not ready for production use. See [docs/todo.md](docs/todo.md) for the
current version and remaining scope.

## Development Environment

Install the following tools using their official installers or your package manager:

| Tool                                    | Requirement                                                | Purpose                                         |
| --------------------------------------- | ---------------------------------------------------------- | ----------------------------------------------- |
| [Rust](https://rustup.rs/)              | Stable, 1.88 or newer; rustfmt and Clippy                  | Rust 2024 workspace                             |
| [Bun](https://bun.sh/docs/installation) | Version in `apps/console/package.json` (currently 1.3.14)  | Console dependencies and scripts                |
| [Node.js](https://nodejs.org/)          | 24 LTS                                                     | Vite 8 and frontend CLI tools                   |
| [Just](https://just.systems/)           | 1.56 or newer                                              | Repository commands                             |
| Python                                  | Python 3                                                   | Asset, service-test, and release-policy scripts |
| Git and OpenSSH                         | Git 2.49 or newer for Node builds; `ssh` and `ssh-keyscan` | Repository checkout and private sources         |

Install the Rust components and Console dependencies from the repository root:

```sh
rustup toolchain install stable --component rustfmt --component clippy
just install console
just --list
```

Use a supported stable Rust toolchain for contributions. The committed manifests
and lockfiles are authoritative for dependency versions. Console uses Bun, Vite 8,
TypeScript, Lit, ESLint, Prettier, Vitest, and Playwright.

Additional services depend on the work you are doing:

- PostgreSQL 15+ is required for the Control API and database regressions.
- Redis 7+ is required for Redis regressions; the API supports an in-memory fallback
  for evaluation.
- Podman or Docker with an accessible engine socket is required for container builds,
  SSR execution, and the Node delivery smoke test.
- Playwright browsers are required for Console browser tests. The backend screenshot
  test separately requires a Chromium executable.

Basic Rust and Console unit tests do not require running the complete platform.

## Local Configuration

The shell examples below use POSIX syntax; use equivalent PowerShell commands on
Windows. From the repository root, copy `config.toml.example` to `config.toml` if
it does not already exist. Configure your development PostgreSQL and Redis endpoints.
These files contain local credentials and are ignored by Git.

For a standalone Node, also copy `node.toml.example` to `node.toml`. Set the Node
token obtained from the setup wizard or administration, the Control API address,
container socket, and writable work/cache directories. Replace example `/data`
paths with directories available to your account. Nodes use the authenticated API
and do not connect directly to PostgreSQL or Redis.

Apply database migrations explicitly when automatic migration is disabled:

```sh
just migrate
```

Do not run development migrations against shared or production infrastructure.
Startup generates a platform secret when the configuration still has the example
key. Keep the generated configuration private.

For Console proxy settings, copy `apps/console/.env.example` to
`apps/console/.env.local` if needed. `VITE_API_TARGET` defaults to
`http://127.0.0.1:7817`; leave `VITE_API_BASE_URL` empty for same-origin requests.
All `VITE_*` values are public build data and must never contain secrets.

## Running in Development

Run each required process in a separate terminal from the repository root:

```sh
just run api
just run console
just run node
```

The API defaults to `http://127.0.0.1:7817`; Console defaults to
`http://localhost:5173`. The Console development server proxies `/api`, `/health`,
and build-log WebSockets to the API. Complete the setup wizard before starting a
Node that needs its token. Open `/ui` on the Console server to inspect shared UI
components.

For trusted local HTTP development, enable `[development].enabled` in the API
configuration or run:

```sh
cargo run -p grass-control-api -- --dev
```

This mode relaxes Secure cookies and authenticated CSRF checks. Keep the API on a
trusted local interface and disable development mode in shared or public deployments.

See [self-hosting](docs/self-hosting.md) for the complete setup, Node registration,
storage, ingress, and deployment walkthrough.

## Building

Use the root Justfile for combined builds:

```sh
just build console   # Type-check and build apps/console/dist
just build           # Console plus development Rust binaries
just build node      # Development Node binary only
just release         # Console plus distributable API and Node binaries
just preview console # Preview the existing Console production build
```

Development Rust binaries use a placeholder for Console assets; run the frontend
server separately. Release binaries embed the current Console output. Direct
`cargo build --release` requires a nonempty `apps/console/dist/index.html` first.
Binaries are written under `target/debug` or `target/release`.

Vite preview does not provide the development API proxy. Use an available API or a
server with a same-origin API and SPA fallback. See the
[Console README](apps/console/README.md) for proxy, preview, and screenshot details.

## Testing and Quality Checks

Run checks relevant to your change. The main commands are:

| Command                      | Coverage                                                                             |
| ---------------------------- | ------------------------------------------------------------------------------------ |
| `just fmt`                   | Format Rust and Console files; modifies files                                        |
| `cargo fmt --all -- --check` | Check Rust formatting without changing files                                         |
| `just check`                 | Rust compilation and Console types, lint, and formatting                             |
| `just clippy`                | All Rust targets with warnings treated as errors                                     |
| `just test`                  | Rust and Console unit tests                                                          |
| `just quality`               | Formatting, Clippy, tests, checks, builds, asset, release-policy, and license checks |
| `just assets-check`          | Console embedding and debug/release rebuild behavior                                 |
| `just release-check`         | Release tag and image metadata policy                                                |
| `just license-check`         | Project license file and package metadata                                            |
| `just test-services`         | PostgreSQL and Redis regressions                                                     |
| `just msrv`                  | Locked all-target compilation with the declared minimum Rust version                 |
| `just audit`                 | Cargo and Bun dependency audits                                                      |

`just fmt`, `just check`, and `just test` accept `rust` or `console` to limit their
scope. For an individual Rust package, use commands such as
`cargo test -p grass-node`. Console commands can also run directly:

```sh
cd apps/console
bun install --frozen-lockfile
bun run check
bun run test
bun run test:install
bun run test:browser
bun run test:e2e
```

`test:browser` selects Chromium; `test:e2e` runs Chromium, Firefox, and WebKit.
Browser tests start their own development server and mock HTTP APIs. Install
browser/system dependencies following Playwright's instructions when necessary.

### Service regressions and schema changes

Ordinary Rust tests leave environment-dependent cases ignored. Configure
`GRASS_TEST_DATABASE_URL` and `GRASS_TEST_REDIS_URL` for explicitly disposable test
services, then run `just test-services`. The command applies migrations to the
configured database, runs the ignored API cases except the Chromium screenshot
case, and runs Redis cache cases. PostgreSQL tests create and remove schemas;
use a dedicated test database, not shared application data. Both environment
variables are required and missing configuration fails before tests run.

For migration or entity changes, verify the migration tracking table and inspect
the actual schema: column types, nullability, defaults, native enum values, indexes,
partial unique indexes, and foreign keys. Scope PostgreSQL catalog queries to the
test schema. Check fresh-schema behavior and upgrade/rollback paths when relevant;
a successful rerun with no pending migrations does not establish that an older
applied migration has the updated shape. Nullable lifecycle timestamps must not
acquire defaults unintentionally.

### MSRV, audits, and CI

Install optional Cargo tools before using their Just commands:

```sh
cargo install cargo-msrv --version 0.18.4 --locked
cargo install cargo-audit --locked
just msrv
just audit
```

Keep the Rust MSRV declaration, Docker builder, CI, and documentation consistent.
Describe failures, skipped tests, and unavailable environments accurately; ignored
service tests are not executed regressions.

PR Quality checks code, unit tests, dependency audits, service regressions, MSRV,
and a real Node container delivery smoke test. Release builds run Quality for the
same commit before building and checking the distributable runtime images. See
[release quality](docs/release-quality.md) for the detailed boundaries.

## Code Conventions

### Rust and shared code

- Rust formatting follows `rustfmt.toml`. Keep imports, declarations, SQL, JSON,
  and mocks readable; use explicit formatting where rustfmt cannot express structure.
- Prefer concrete types, direct functions, and clear responsibility boundaries.
  Avoid generic service layers or helper abstractions without an actual need.
- Preserve wire contracts, error meaning, authorization, tenant isolation, Host
  binding, preview grants, and artifact/archive/log path traversal protections.
- Tests should cover observable behavior and meaningful regressions. Preserve
  existing assertions and fixtures during structural refactors.

### Control API

- Feature modules follow the URL tree. Each file owns one path and its HTTP methods,
  DTOs, validation, and orchestration. Routers compose immediate children; preserve
  exact paths and trailing-slash behavior. Map `{project_id}` to `by_project_id.rs`.
- Keep endpoint contracts local. Share protocol primitives and genuine business
  capabilities, rather than extracting DTOs solely to remove duplication. Use typed
  responses; reserve open JSON values for intentionally open metadata/configuration.
- Shared business capabilities belong in `domain`; provider clients, storage, and
  other technical adapters belong in `infra`. A route may use SeaORM directly.
  Keep authorization explicit and use the narrowest necessary visibility.
- Nullable PATCH fields distinguish missing, null, and a supplied value with local
  `Option<Option<T>>`, `serde(default)`, and the `infra::http::patch::nullable`
  deserializer. Test raw JSON and resulting updates. Validate Node contracts against
  `grass-node-protocol` and preserve existing database state representations.
- `infra/audit` owns audit types, redaction, persistence, logging, queries, and
  retention. Preserve actor/target context, visibility, and request IDs. Critical
  mutations and their business audit share a transaction; audit failure rolls it back.
  Emit success logs after commit. Request-audit failure must not replace a completed
  response; external actions record actual outcomes. Database and log delivery are
  not atomic. Redact credentials, tokens, cookies, and keys before either sink.
- Distinguish business rejection from infrastructure failure and retain safe error
  context. Unknown persisted state must not silently become valid. Place local tests
  after production items, shared fixtures in `src/test_support`, and HTTP scenarios
  in `src/integration_tests`; do not expose feature test modules as fixture APIs.

### Console

Follow [CODE_STYLE.md](apps/console/CODE_STYLE.md) and
[DESIGN.md](apps/console/DESIGN.md). Keep strict TypeScript, typed API boundaries,
business requests in services, and subscription cleanup on component disconnection.

## Files and Documentation

Use the existing directory structure: applications under `apps`, shared Rust
libraries under `crates`, technical references under `docs`, and validation tools
under `scripts`. Rust modules use snake_case; Console directories use kebab-case
and follow the component/file naming rules in its code-style guide.

Commit dependency lockfile updates with the dependency changes that require them.
Do not hand-edit lockfiles. Exclude secrets, local configuration, temporary files,
`target`, `node_modules`, `dist`, coverage, screenshots, and browser reports.
Retain third-party license and attribution notices when incorporating code or assets.

Link to detailed references instead of duplicating their contents.

## Workflow Conventions

1. After any code change, review all repository documentation for affected behavior,
   commands, configuration, APIs, and examples, and update every affected document.
   This includes the root and application READMEs, this guide, agent instructions,
   operational guides, and example configuration. Code and documentation must remain
   consistent.

## Commit Messages

Use Conventional Commits with a required scope and an uppercase first letter in
the description:

```text
<type>(<scope>): <Description>
```

Allowed types are `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `ci`, and `build`.
Examples: `fix(auth): Reject invalid CSRF tokens` and
`docs(contributing): Document local development`.

Keep each commit focused and independently revertible. Separate unrelated formatting
or generated changes, and describe the semantic change rather than an internal
priority or phase identifier. Reference a related issue when useful.

## Pull Requests

Use English for Issue and PR titles and descriptions. PR titles follow the commit
format above. Describe the problem and resulting behavior, with these sections:

- **Summary**: the change and its purpose, including configuration or API implications.
- **Validation**: commands run, results, and any skipped or unavailable checks.
- **Tracking**: related issues and parent issues when applicable.
- **Known Limitations**: material risks or follow-up work, when applicable.

Use `Closes #<number>` only for issues fully completed by the PR. Avoid internal
priority, phase, or milestone-slice identifiers in titles and descriptions; use
GitHub tracking metadata instead. Target `main` unless the contribution belongs to
another agreed integration branch. Keep changes and documentation focused enough
to review, and satisfy the applicable CI checks.

## License and Copyright

Grass Worker is licensed under the [BSD 3-Clause License](LICENSE).

Copyright (c) 2026, Grass Development Team.

Contributions are provided under the same license. Submit only code and assets you
have the right to contribute, and preserve any applicable third-party notices.
Source redistributions must retain the copyright notice, license conditions, and
disclaimer; binary redistributions must reproduce them in accompanying materials.
The copyright holder's and contributors' names may not be used to endorse derived
products without prior written permission. The full license governs permissions
and the warranty disclaimer.
