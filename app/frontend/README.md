# frontend

Minimal Bun-powered placeholder app.

## Commands

```bash
bun test
bun run dev
bun run build
```

## Build Output

The frontend build writes into `../../crates/assets/assets/public/` so the shared assets crate can embed release assets and `app/api` can serve them when no runtime `./public/` directory is present.
