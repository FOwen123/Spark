# Contributing to Spark

Thanks for helping. This page covers setup, the checks every change must pass, and the rules that keep Spark consistent.

## Setup

```sh
bun install
bunx playwright install chromium webkit
bun run build
```

Run `./target/debug/spark serve` and open the printed URL. For UI work, also run `bun run dev` and use the Vite URL; it proxies `/ui`, `/api` and `/assets` to the server (set `SPARK_URL` if the server isn't on the default port, and `SPARK_ALLOW_ORIGIN` to the Vite origin).

## Before you commit

```sh
bun run verify
```

It runs rustfmt, clippy with warnings as errors, the Rust tests, TypeScript, Biome lint, Vitest, the web build and the Playwright suite in Chromium and WebKit. Everything must pass. If a check fails, fix the cause; don't skip, disable or loosen a test to get green.

Optional runs:

- `bun run bench` measures the performance budgets (see [docs/PERFORMANCE.md](docs/PERFORMANCE.md)).
- `bun run screenshots` rebuilds the README screenshots and `fixtures/demo/Showcase.spark`.

Commit in small steps with [Conventional Commit](https://www.conventionalcommits.org) messages, for example `feat(core): …`, `fix(mcp): …`, `test(e2e): …`.

## Rules

- **Rust owns the document.** The UI sends intents and renders the patches it gets back; it never changes document state itself. New editing features are a command in `spark-server` that produces ops from `spark-core`.
- **Black and white only** in Spark's own interface. Use the tokens in `web/src/styles/tokens.css` (exposed as Tailwind colors like `bg-panel` and `text-fg-2`); the default hued Tailwind palette is turned off on purpose. Show state with weight, fill, outline and dashes, never color.
- **Independent implementation.** Write code, tool descriptions and guides in your own words, and add only openly licensed assets, with their license files next to them.
- **Generated demo content** for fixtures and screenshots. `bun run screenshots` and `spark demo` make it for you.
- **Tests that matter.** Cover behavior users and agents rely on. Tests run against throwaway directories: agent config writers use `fixtures/agents`, never your real configs.

## Pull requests

Open a PR against `main` with one focused change. Say what changed and how you tested it, and add before and after screenshots for anything visible. If you work with a coding agent, [AGENTS.md](AGENTS.md) gives it the same rules.

## Builds and releases

`.github/workflows/build.yml` runs `bun run verify` on every pull request, then builds the Mac app for Apple Silicon and attaches an unsigned `.dmg` to the run.

To release:

1. Set the new `version` in the root `Cargo.toml` and merge it.
2. Push a matching tag: `git tag v0.2.0 && git push origin v0.2.0`.
3. The workflow signs and notarizes the app, then puts the `.dmg` in a draft release. Review it and publish.

Signing needs a Developer ID Application certificate and an app-specific password for notarization, stored as repository secrets: `APPLE_CERTIFICATE` (the `.p12`, base64), `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD` and `APPLE_TEAM_ID`. Without them, a tag still builds, but the app is unsigned.

## Where things are

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how the pieces fit, and [docs/DECISIONS.md](docs/DECISIONS.md) for choices made where the spec left room.
