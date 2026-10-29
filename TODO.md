# Spark roadmap

How Spark v1 was built, milestone by milestone. `docs/SPEC.md` defines the behavior each item delivers, and its "Out of scope for v1" section lists what comes later. When a change completes a roadmap item, tick it in the same commit.

## M0 · Scaffold
- [x] Cargo workspace: `crates/spark-core`, `crates/spark-server`, `crates/spark-cli` (binary `spark`)
- [x] `web/`: Vite + React + TypeScript + Tailwind v4, Bun workspace at repo root
- [x] `apps/desktop`: Tauri v2 shell that starts the server in-process and loads the UI
- [x] `bun run verify` script (fmt, clippy, cargo test, tsc, lint, vitest, web build, Playwright e2e) and a trivial e2e that opens the UI served by `spark serve`
- [x] README: what Spark is, build and run instructions, project layout

## M1 · Core document
- [x] Ids (`<base36>-<site>` nodes, `p-` pages, ULID files), fractional-index sort keys
- [x] Node kinds, typed style model + raw CSS map, OKLCH colors, tokens (Tailwind v4 namespaces, aliases)
- [x] Op set (insert, delete/tombstone, move, set, page, token, asset ops) with validation
- [x] Undo/redo: inverse-op transactions, grouping, 250 ms merge, 250-entry cap
- [x] `.spark` SQLite persistence: incremental, atomic autosave, schema version + migrations, embedded assets
- [x] CSS compilation of node styles; `spark export --json`
- [x] Unit tests for all of the above; benchmark: 20k-node file opens in under 1 s

## M2 · Canvas and UI shell
- [x] WebSocket `/ui` channel with token auth: snapshot on connect, op patches, intents, batched measured rects
- [x] B&W tokens (`web/src/styles/tokens.css`), light and dark, Geist fonts bundled
- [x] App bar with tabs, home screen (recent, new, open), left panel (pages + layer tree), vertical toolbar, right inspector shell
- [x] DOM node renderer, `matrix3d` camera, pan and zoom, artboard labels, culling
- [x] HUD canvas: hover, selection, handles, marquee
- [x] e2e: create file, see artboard, pan and zoom, reload persists

## M3 · Editing
- [x] Tools: move, frame, rectangle, text (inline editing), image, hand
- [x] Select, multi-select, deep-select, marquee, move, Option-drag duplicate, resize with modifiers, nudge
- [x] Snapping and spacing guides via Rust R-tree
- [x] Inspector: position/size (fixed, fill, hug), layout (freeform/flex controls), appearance, fill (solid, gradient, image), border, shadow, typography, effects
- [x] Layer tree: rename, reorder/reparent by drag, hide, lock
- [x] Context menu and the full keyboard shortcut set from the spec
- [x] Clipboard: Spark format, HTML with inline styles, SVG, images, copy as HTML/JSX/Tailwind
- [x] Tokens view: edit, alias, export CSS
- [x] e2e: draw, edit in inspector, undo/redo, copy-paste, persistence

## M4 · MCP server
- [x] Streamable HTTP `/mcp` with bearer token, Host/Origin checks, on-demand sessions, `/mcp/config.json`
- [x] File routing by id, route, link or path; result header with FNV-1a-32 token hash
- [x] Read tools: basic info, selection, node info, children, tree summary, JSX (inline + Tailwind), computed styles, find nodes, screenshot (UI render bridge with call ids + timeout), fill image, font info
- [x] Write tools: create artboard (auto-placement), write_html (Rust HTML + inline CSS parser, clone tag, layer-name, images), update styles, set text, rename, duplicate, move, delete, insert_image
- [x] Pages, files and tokens tools; honest tool annotations
- [x] Guides written for Spark (`docs/guides/*.md`, served by `get_guide`) and server instructions
- [x] Agent presence: agents in app bar, tab activity, falling-dot indicator, read/write sweeps, finish_working_on_nodes
- [x] Each agent tool call is one undo transaction
- [x] e2e: MCP client builds an artboard via write_html and it appears live; get_jsx round-trip; screenshot returns a PNG

## M5 · Agent connectivity
- [x] `spark mcp` stdio relay (local initialize/tools/list, friendly offline result, `--headless`)
- [x] `spark connect|disconnect <agent> [--dry-run]` for Claude Code, Codex, Cursor, VS Code/Copilot, OpenCode, Gemini CLI; `.mcpb` for Claude Desktop
- [x] "Connect your agent" UI on home and in the agent panel
- [x] Agent panel: detect agent CLIs, run a prompt headless with Spark MCP attached, stream output, show tool-call log
- [x] Tests: config writers against fixture files (JSONC comments and TOML formatting preserved, idempotent, 0600); e2e of the connect UI

## M6 · Images
- [x] Content-addressed assets, image fills, `spark-asset://` limited to granted directories
- [x] Image requests: `spark-gen://agent` placeholders, `list_image_requests`, `fulfill_image_request`
- [x] Provider trait + adapters: OpenAI, Gemini, fal.ai, Replicate, OpenRouter, ComfyUI/Automatic1111
- [x] Settings: enable providers, keys in OS keychain, per-call cap, monthly budget
- [x] Async generation state on nodes and in `get_node_info`
- [x] e2e: image request placeholder → fulfil → visible; provider path covered with a mock HTTP provider

## M7 · Export
- [x] PNG/JPG/WebP rasterization at scale or target size, SVG from Rust, PDF (single and combined)
- [x] Export presets in the inspector; `spark export` CLI; MCP `export` and `export_combined_pdf`
- [x] e2e: export each format and validate the output files

## M8 · Desktop and performance
- [x] Tauri: deep links, single instance, file association, native menus, tabs, window state, no background throttling for agent-active documents
- [x] Performance pass against the spec targets with a recorded benchmark in `docs/PERFORMANCE.md`
- [x] Final docs: README screenshots (made from generated demo content), CONTRIBUTING, architecture notes
