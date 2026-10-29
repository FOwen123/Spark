# Agent guide

This repo holds Spark, a local-first design tool with a Rust document core, a React canvas, a Tauri desktop shell and an MCP server for AI agents. `docs/SPEC.md` defines what Spark does, `docs/ARCHITECTURE.md` maps the code, and `docs/DECISIONS.md` records choices the spec left open.

Pick the parts of this guide that fit the task:

- Reviews, explanations and investigations need no commit.
- Code changes get the checks below plus evidence of what changed. For a bug, reproduce it in a failing test before you fix it.
- Documentation edits get a careful read against the code they describe. Run `bun run verify` only when you touch `docs/guides/` (see Project rules).

## Workflow

1. **Read.** Find the behavior in `docs/SPEC.md` and the code in `docs/ARCHITECTURE.md`. If `docs/DECISIONS.md` covers the area, follow it, or update the entry when you change the choice.
2. **Build.** An editing feature is a command in `crates/spark-server` that produces ops from `crates/spark-core`; the UI in `web/` sends the intent and renders the patch it gets back.
3. **Prove.** Run `bun run verify`. Cover user-visible behavior with a Playwright test in `e2e/`, and check UI changes in light and dark.
4. **Ship.** Commit with a Conventional Commit message (`feat(core): …`, `fix(mcp): …`, `test(e2e): …`, `docs: …`). Push, open a PR or tag a release only when asked.

## Project rules

- **Rust owns the document.** UI code sends intents and renders patches; it never changes document state itself. `Document::apply` is the only thing that mutates a document.
- **Spark's own interface is black, white and gray.** Use the tokens in `web/src/styles/tokens.css` (Tailwind colors like `bg-panel` and `text-fg-2`); the hued Tailwind palette is turned off on purpose. Show state with weight, fill, outline and dashes. User designs can use any color.
- **Spark is an independent implementation.** Write code, tool descriptions and guides in your own words. Bundle only openly licensed assets (Geist fonts, Lucide icons) and keep their license files beside them.
- **Fixtures and screenshots come from generated demo content.** `bun run screenshots` rebuilds the README images and `fixtures/demo/Showcase.spark` from `fixtures/demo/showcase.ts`; `spark demo <path> --nodes <n>` writes large test files.
- **Tests never touch the machine's real state.** Agent config writers run against `fixtures/agents/home`, provider keys go to `SPARK_KEYRING_FILE` instead of the keychain, and files go to temp dirs. When you run `spark` by hand, point `SPARK_CONFIG_DIR`, `SPARK_FILES_DIR`, `SPARK_HOME_DIR` and `SPARK_KEYRING_FILE` at a `mktemp -d` directory, as `e2e/global-setup.ts` does.
- **`docs/guides/*.md` ships in the binary.** `get_guide` serves these files to agents through `include_str!` in `crates/spark-server/src/mcp/tools.rs`, so editing one changes product behavior. A new guide needs a match arm there and a line in the spec's tool table.
- **Performance budgets are tested.** `e2e/perf.spec.ts`, `crates/spark-core/tests/bench.rs` and `crates/spark-server/tests/mcp_perf.rs` assert the targets in `docs/PERFORMANCE.md`. After changing a hot path (document load, op apply, MCP reads, canvas rendering), run `bun run bench` and update the numbers there.

## Checks

`bun run verify` runs everything a change must pass, stopping at the first failure: rustfmt, clippy with `-D warnings`, the Rust tests, TypeScript, Biome lint, Vitest, the web build, the `spark` binary build and the Playwright suite in Chromium and WebKit. If a check fails, fix the cause; never skip, disable or loosen a test to get green.

Faster loops while you work:

- `cargo test -p spark-core` or `cargo test -p spark-server --test mcp` for one crate or test file.
- `bun run --cwd web test` for Vitest.
- `bun run build && bunx playwright test e2e/<file>.spec.ts --project chromium` for one e2e file. Playwright drives `target/debug/spark` serving the built `web/dist`, so rebuild first or it tests stale code.

Use `bun run <script>` for the scripts in `package.json`. Plain `bun build` and `bun test` are Bun's own bundler and test runner and skip this repo's setup. First-time setup needs `bun install` and `bunx playwright install chromium webkit`.
