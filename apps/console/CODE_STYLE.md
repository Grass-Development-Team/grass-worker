# Console Code Style

These conventions apply to TypeScript, JavaScript, HTML, CSS, JSON, and Markdown in this directory. Prettier controls formatting; ESLint and TypeScript check code issues.

## Automatic formatting

`.prettierrc.json` is the authoritative formatting configuration:

| Setting                              | Value                |
| ------------------------------------ | -------------------- |
| Print width                          | 100                  |
| Indentation                          | 2 spaces             |
| Tabs                                 | Disabled             |
| Semicolons                           | Required             |
| Strings                              | Double quotes        |
| Trailing commas                      | Everywhere supported |
| Arrow-function parameter parentheses | Always               |
| Line endings                         | LF                   |

Use `bun run fmt` to format files and `bun run check` to validate them. Prettier formats Lit `html` and `css` templates. Print width is a formatting guideline; do not split URLs, strings, or expressions in ways that change their meaning.

Dependencies, build output, coverage, browser-test artifacts, and the lockfile are excluded through `.prettierignore`. Bun manages the lockfile.

## Files, imports, and naming

- Use kebab-case directory names. Component directories typically contain `index.ts` and `style.css`.
- Use PascalCase for classes and interfaces, camelCase for variables, functions, and properties, and UPPER_SNAKE_CASE for clearly defined fixed constants.
- Prefix custom elements and public events with `gw-`. Use kebab-case event names.
- Use `@/` for imports across directories within `src`. A component's own files may use relative imports.
- Use `import type` for type dependencies to avoid unused imports and unintended runtime code.
- Prettier does not sort imports. When editing imports, merge imports from the same module and remove unused entries.

## TypeScript and models

Keep strict mode enabled. Do not use `any`; use `unknown` for external data and validate and narrow it at boundaries. Describe DTOs with interfaces and types rather than introducing model inheritance without a business purpose.

Preserve the backend contract's snake_case API fields. Use frontend naming conventions for presentation and interaction state. Give requests, responses, pagination, and errors explicit types.

Use type assertions for validated boundary adaptations. Do not use them to hide unhandled responses, errors, or type diagnostics.

## Lit

Declare reactive properties explicitly. Do not reflect internal state to HTML; reflect only public properties that need it. The current configuration uses `useDefineForClassFields: false` with Lit property accessors.

```ts
import { LitElement, html } from "lit";

export class Example extends LitElement {
  static properties = {
    disabled: { type: Boolean, reflect: true },
    items: { attribute: false },
  };

  disabled = false;
  items: string[] = [];

  render() {
    return html` <gw-button ?disabled=${this.disabled}>${this.items.length} items</gw-button> `;
  }
}
```

Pass complex objects through `.property`, boolean attributes through `?attribute`, and events through `@event`. Keep templates semantically complete and extract complex conditions and data preparation into named methods.

Use `unsafeCSS` only for local, static CSS imports. Do not insert API content into HTML, SVG, or dynamic styles. Let Lit escape text normally.

Components that need form behavior retain native controls and implement form association. A page's native `<form>` defines its submission and validation boundary; do not bypass validation with click handlers.

## CSS

Components use Shadow DOM and their own styles. Page layouts use explicit structural classes. Use the tokens specified in `DESIGN.md` for colors, dimensions, corner radii, and motion.

Express layouts with flex/grid and `gap`. Avoid accumulating one-off styles, business conditions, or deeply nested selectors in components. Limit inline styles to dynamic geometry derived from explicit data and small amounts of local layout styling.

## Errors and lifecycle

Treat caught exceptions as `unknown` and use the shared error helper to obtain a display message. Surface errors through fields, pages, or operation feedback; do not silently swallow business failures. Cases with explicit fallback behavior, such as branding fallbacks or malformed log frames, may retain that behavior.

Before writing asynchronous results into state, check whether the component is still valid, whether the session version is current, and whether the target resource is unchanged. On disconnection, release subscriptions, timers, listeners, observers, object URLs, and connections.

## Validation

Unit tests cover API contracts, serialization, permissions, safe redirects, race conditions, and log catch-up. Browser tests cover forms, focus, keyboard interactions, routing, theme transitions, and responsive layouts.

Use mocked services in tests. Assert observable business results rather than only checking internal implementation details. Before handing over changes for review, run `bun run check`, `bun run test`, and the relevant browser tests.
