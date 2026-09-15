# Release quality checks

## Console types

Use `just check console` (or `vp check` in `apps/console`) for formatting, linting and type checking. Vite+ enables its type-aware checker through `lint.options.typeAware` and `lint.options.typeCheck`. Type errors fail this command and the shared repository quality gate.

`vp exec tsc --noEmit` is the independent TypeScript compiler check. The project retains strict checking and includes Vite client, Node and Vite+ test global types. React and React DOM declarations are explicit development dependencies.

The type-checking change repairs incomplete test fixtures, unsupported component variants, concrete API filter shapes and mismatched announcement timestamps and mutation return types. Both checkers were verified with a temporary incompatible assignment, which failed with TS2322; the probe was removed afterwards.

Enabling the type-aware lint engine also surfaces advisory warnings in existing frontend code. They remain visible and are not suppressed; frontend architecture and advisory lint cleanup are deferred from this Rust cleanup round. Type errors are blocking.

## Minimum supported Rust version

The workspace declares Rust 1.88. `just msrv` uses cargo-msrv to verify every workspace target against that declaration, including test targets, with the committed lockfile and default application features. Install cargo-msrv 0.18.4 with `cargo install cargo-msrv --version 0.18.4 --locked` when it is unavailable.

To find the minimum again after dependency or language changes:

```sh
cargo msrv find --manifest-path apps/control-api/Cargo.toml --min 1.85 --no-log -- cargo check --workspace --all-targets --locked
just msrv
```

The lower search bound corresponds to the workspace's Rust 2024 edition. Preserve the lockfile during the search. An unavailable toolchain or network failure is a validation failure, not evidence that an older compiler is incompatible.

On macOS ARM64, cargo-msrv tested Rust 1.90.0 successfully, rejected 1.87.0 because locked SeaQuery/time dependencies require 1.88, and passed 1.88.0. CI verifies the declared version on Linux x86_64 and macOS ARM64. Docker already uses Rust 1.88. Updating the minimum requires keeping the workspace declaration, Docker builder and self-hosting documentation aligned.

## Embedded Console assets

`just build` builds development binaries; run the Console separately with `just run console`. `just release` builds the Console and then produces distributable binaries under `target/release`. Direct `cargo build --release` requires a nonempty `apps/console/dist/index.html`, produced by `just build console` first.

The asset build script watches the complete dist directory and copies resources into its profile/target-specific `OUT_DIR`. It does not generate files in the source tree. Missing or empty release HTML fails the build. Debug builds always use the development-server placeholder.

`just assets-check` exercises the actual asset crate in a disposable fixture workspace: changed HTML, added and removed resources, alternating debug/release profiles, missing/empty dist HTML and a missing dist directory. It is included in `just quality` and CI. Its dependency/build cache lives under the selected Cargo target directory.

## PostgreSQL and Redis regressions

`just test rust` runs the fast suite and reports environment-dependent tests as ignored. `just test-services` requires `GRASS_TEST_DATABASE_URL` and `GRASS_TEST_REDIS_URL` for disposable test services. It exits before running any command when either variable is absent. Do not point it at production databases or shared infrastructure: the PostgreSQL tests create and remove their own test schemas.

The service suite applies current migrations using a temporary runtime configuration, runs every ignored Control API test except the Chromium screenshot case, and runs all ignored Redis cache tests. This currently covers 30 PostgreSQL-related cases, one standalone Redis session authorization case and three Redis cache cases. It includes authentication-version shape/revocation, region backfill, domain onboarding, lifecycle transactions, upgrade/rollback and native schema assertions.

CI provides disposable PostgreSQL 17 and Redis 7 services in a dedicated job. The Node Docker smoke test and Chromium screenshot test retain their separate runtime requirements; Chromium is not counted as an executed database regression.
