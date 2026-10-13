# Grass Worker Console

Console uses Bun, Vite 8, TypeScript, and Lit Web Components. Run all frontend commands from this directory.

## Quick start

Install Node.js 24 LTS and the Bun version declared in `package.json`, then run:

```bash
cd apps/console
bun install --frozen-lockfile
bun run dev
```

Open the address printed in the terminal, which defaults to `http://localhost:5173`. The development server proxies `/api`, `/health`, and build-log WebSockets to the Control API. The default target is `http://127.0.0.1:7817`. Business pages require a running Control API; an empty database starts the setup wizard.

If the API runs at a different address, create `.env.local` in this directory:

```dotenv
VITE_API_TARGET=http://127.0.0.1:7817
```

`VITE_API_TARGET` controls the development proxy only. `VITE_API_BASE_URL` specifies the API base URL used by the browser. Leave it empty for same-origin requests. Cross-origin deployments require appropriate server-side Cookie, CORS, and CSRF configuration. Frontend environment variables are included in public build output and must not contain credentials.

## Scripts

| Command                | Purpose                                             |
| ---------------------- | --------------------------------------------------- |
| `bun run dev`          | Start the development server                        |
| `bun run build`        | Check types and generate `dist/`                    |
| `bun run preview`      | Preview an existing build locally                   |
| `bun run check`        | Run TypeScript, ESLint, and Prettier checks         |
| `bun run lint`         | Run ESLint separately                               |
| `bun run fmt`          | Format files with Prettier                          |
| `bun run test`         | Run service and infrastructure unit tests           |
| `bun run test:watch`   | Run unit tests in watch mode                        |
| `bun run test:install` | Install Chromium, Firefox, and WebKit test browsers |
| `bun run test:browser` | Run Chromium browser tests                          |
| `bun run screenshots`  | Capture every route with mock data                  |
| `bun run test:e2e`     | Run all three browser projects                      |

Before running browser tests for the first time:

```bash
bun run test:install
bun run test:browser
```

Browser tests start a separate development server automatically and use mocked HTTP APIs. They do not connect to real databases, nodes, or storage services. If Playwright reports missing system dependencies, follow its installation instructions and retry.

To preview a production build:

```bash
bun run build
bun run preview
```

Vite preview does not inherit the development proxy. Point `VITE_API_BASE_URL` at an available API, or deploy `dist/` to a server that provides a same-origin API and SPA fallback. The existing Control API asset-embedding entry point reads this directory's `dist/` output.

The root Justfile, CI, and Docker build use the same Bun scripts and committed lockfile. See [CONTRIBUTING.md](../../CONTRIBUTING.md) for repository-wide setup and contribution conventions.

## Structure and development

- `assets/icons`, `assets/images`: local visual assets.
- `src/ui/components`: shared Lit component library.
- `src/ui/widgets`: business components composed from shared UI components.
- `src/ui/layouts`: authentication, Console, and project layouts.
- `src/ui/pages`: pages and centralized route definitions.
- `src/services`: APIs, models, and business state organized by domain.
- `src/infra`: requests, serialization, queries, routing, themes, and lifecycle controllers.
- `tests/browser`: browser regressions for components, routes, and key workflows.

See [DESIGN.md](./DESIGN.md) for the design specification and [CODE_STYLE.md](./CODE_STYLE.md) for coding conventions. In development, open `/ui` to inspect shared component variants and states.

All single-line form controls share a 48px height. Authentication submit buttons are centered at their content width on desktop and fill the form width on mobile, including MFA verification. Console supports email/password sign-in and registration; it does not expose third-party sign-in or identity-provider management.

Project creation uses a dedicated header and a two-column workspace for project details and framework presets. Expand **Build settings** to customize the runtime, root directory, commands, and output directory before submission. Narrow screens stack the form, presets, and creation action.

To add a page, place it in the corresponding business directory and register its route, permissions, layout, and dynamic import in `src/ui/pages/index.ts`. Keep business requests in service modules; components do not request business APIs directly.

## Screenshot review

Start the frontend on the capture port in one terminal:

```bash
bun run dev --host 127.0.0.1 --port 4173 --strictPort
```

Then generate the gallery in another terminal:

```bash
bun run screenshots
```

The capture tool reads the route registry, expands parameterized pages, and includes setup steps and MFA states. It generates desktop light, desktop dark, and mobile light screenshots with mock API data. Open `screenshots/index.html` locally to browse them; `screenshots/manifest.json` records route coverage and image paths. Generated screenshots are ignored by Git.

Use `CONSOLE_SCREENSHOTS_URL` to select a different running frontend address and `CONSOLE_SCREENSHOTS_DIR` to change the output directory. Install the Playwright Chromium browser with `bun run test:install` before the first capture.
