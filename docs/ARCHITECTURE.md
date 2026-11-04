# Architecture

Spark is three Rust crates, a React UI and a Tauri shell. The document lives in Rust; the UI is a view that renders it and sends intents.

```
 browser or Tauri webview                     agents (Claude Code, Codex, …)
 ┌──────────────────────────┐                   │ stdio            │ HTTP
 │ web/ (React)             │              spark mcp (relay)       │
 │  DocStore mirror ◀─patch─┼──┐                │                  │
 │  canvas, HUD, panels ──intent─┐             ▼                  ▼
 └──────────────────────────┘  │ │   ┌───────────────── spark-server (axum) ─────────────────┐
                               │ └──▶│ /ui WebSocket ─ commands ─┐      /mcp ─ tools ─┐       │
                               └─────│ patches, renders,         ▼                    ▼       │
                                     │ agent activity       Hub: open documents, agents,     │
                                     │                      render bridge, settings          │
                                     └───────────────────────────┬──────────────────────────┘
                                                                 ▼
                                      spark-core: Document, Op, Editor (undo), Store (SQLite),
                                      CSS compile, HTML import, code and SVG export
```

## spark-core

- **Document** (`doc.rs`): a flat map of nodes plus a `(parent, sort key)` entry per node, pages, tokens and content-addressed assets. Node ids are `<base36 counter>-<site>`; sort keys are fractional indexes (`fractional.rs`), so reordering rewrites one string.
- **Ops**: `Insert`, `Delete` (a tombstone), `Move`, `Set { node, prop, value }`, page, token and asset ops. `Document::apply` validates an op, applies it and returns its inverse. Nothing else mutates the document.
- **Editor** (`history.rs`): applies op batches atomically and keeps undo history (grouping for drags, a 250 ms merge window for same-key edits, 250 entries).
- **Styles** (`style.rs`, `css.rs`): a typed subset of CSS for layout plus a design layer (fills, border, radius, shadows, effects, typography), with a raw map for CSS Spark doesn't model. `css::compile` turns a node into CSS; `apply_decl` parses CSS into the typed model. Colors are stored as OKLCH.
- **Store** (`store.rs`): a `.spark` file is SQLite (WAL) with `meta`, `pages`, `nodes`, `tree`, `tokens`, `assets`, `ops` and a `layout` cache. Saves write only dirty rows; schema upgrades run as migrations.
- **HTML import and export** (`html.rs`, `codegen.rs`, `svg.rs`): HTML with inline styles becomes nodes; nodes become HTML, JSX, Tailwind JSX or SVG.

## spark-server

- **Hub** (`hub.rs`): open documents, connected UI clients, agents (MCP sessions), the screenshot render bridge, and the reveal step used by links and opened files. Autosave runs on every change, plus a one-second flush that also checkpoints the WAL once edits pause.
- **UI channel** (`ui.rs`, `commands.rs`, `edit.rs`, `views.rs`): one WebSocket per window. Clients open files (and get a snapshot with layout rects), send commands (create, set styles, drag and resize sessions with R-tree snapping, paste, export…), selections and measured rects, and receive patches. File-list requests (`browse`, `mkdir`, `move`, `duplicate`, `rename`) work on the recent list and the files dir without opening a tab; `Config::folder` is the one place a folder path is checked against escaping the files dir.
- **MCP** (`mcp/`): Streamable HTTP at `/mcp` with a bearer token, Host and Origin checks, and sessions created on demand. Every file-scoped result starts with a header naming the file and hashing its tokens; each write tool call is one undo step. Agent presence (who is connected, which artboards they are working on, read and write sweeps) goes to the UI.
- **Rendering and export** (`export.rs`, `layout.rs`): screenshots and raster exports render in a connected UI (off screen); without one, Rust renders its own SVG with resvg. PDFs go through krilla. When nothing has measured a page, taffy lays it out with real font metrics.
- **Fonts** (`fonts.rs`): installed fonts, plus any Google Fonts family a design names. `/fonts/list` names the installed and Google families for the UI's font picker; `/fonts/css?family=…` downloads its `@font-face` rules once and points them at `/fonts/file/…`, which downloads each font file on first use; both are cached in the config dir.
- **Agents and images** (`relay.rs`, `connect.rs`, `runner.rs`, `providers.rs`, `generate.rs`): the stdio relay, config writers for each agent, the agent panel's CLI runner, image requests, and provider adapters behind a cost guard. Provider keys live in the OS keychain.
- **Security**: the server binds 127.0.0.1, every `/ui` and `/mcp` request needs the token stored in the config dir with mode 0600, foreign `Origin` headers are refused, and agents can only read images from folders the user granted.

## web

- `lib/doc.ts` is a read-only mirror of a file, updated from snapshots and patches, with per-node subscriptions so a patch re-renders only what changed.
- `components/Canvas.tsx` renders the page under one `matrix3d` camera, measures layout back to the server once per frame, mounts artboard contents only when on screen and large enough (level of detail), and drives pointer interactions. A single HUD canvas draws selection, handles, guides, labels and agent activity. `components/canvasOverlays.ts` holds the pixel grid, layout guides, flex padding/gap areas and handles, and Alt-measurement drawing; `lib/view.ts` holds the browser-side view toggles.
- `lib/fonts.ts` adds a `/fonts/css` stylesheet for every font family the open file names, so Google Fonts draw on the canvas and in screenshots.
- `lib/prefs.ts` holds per-device UI preferences in local storage (canvas behavior, nudge steps, panel visibility, view toggles); `components/Tip.tsx` is the tooltip and keycap component, `components/ZoomMenu.tsx` the inspector's zoom and preferences menu.
- `lib/eyedropper.ts` picks a screen color: the native `EyeDropper` where it exists, otherwise a click overlay that renders the artboard with `renderToPng` and reads the pixel (WKWebView has no `EyeDropper`).
- `components/inspector/` edits styles through `setStyle` (CSS in, ops out) and `set` (structured values like fills).

## apps/desktop

The Tauri shell starts the server in-process and opens the UI from it, so the desktop and browser builds share one transport. It adds native menus, `spark://` deep links, `.spark` file association, single-instance forwarding and window state, and its binary doubles as the MCP relay (`<app> mcp`).
