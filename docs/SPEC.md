# Spark specification

Spark is a local-first, open-source design tool for people who design with AI agents. Designs are real HTML and CSS, so what you draw is what ships. Agents connect over MCP, read and write the canvas, and watch their work appear live. The interface is strictly black, white and gray.

This document defines what Spark does and how it is built. `TODO.md` breaks it into milestones.

## Principles

1. **Local-first.** Files live on disk as `.spark` files. No account, no cloud, works offline.
2. **Rust owns the truth.** The document, edit operations, undo, persistence, HTML parsing, code export and the MCP server are Rust. The web UI is a view that renders the document and sends user intents.
3. **Designs are web standards.** Nodes render as real DOM elements with real CSS (flexbox, text, fonts). Export to HTML/JSX/Tailwind is a direct translation, not a guess.
4. **Any agent, any image model.** Spark never requires a specific AI vendor. Agents connect through standard MCP. Image generation is pluggable and bring-your-own-key, and an agent can fulfil image requests itself.
5. **Fast.** Targets: open a 20,000-node file in under 1 s; apply an edit op in under 1 ms; MCP read tools answer in under 10 ms p50 (excluding screenshots); pan and zoom at 60 fps on a 5,000-node page.
6. **Minimal black and white.** No hue anywhere in Spark's own chrome. State is shown through weight, fill, outline and dash, never color. User designs can of course use any color.

## Architecture

```
┌──────────────── Spark desktop (Tauri v2) ────────────────┐
│  window, tabs, menus, spark:// deep links, single instance│
│  ┌──────────── web UI (React + TS, Vite) ──────────────┐ │
│  │ canvas (DOM nodes + HUD <canvas>) · panels · tools  │ │
│  └───────────────▲─────────────────────┬───────────────┘ │
│     WebSocket: snapshot + patches      │ intents, measured│
│                  │                     ▼ rects, renders   │
│  ┌──────────────────── spark-server (axum) ─────────────┐│
│  │ /ui (WebSocket)   /mcp (MCP Streamable HTTP)         ││
│  │ /mcp/config.json  /assets/<hash>                     ││
│  └───────────────────────┬──────────────────────────────┘│
│                          ▼                                │
│  spark-core: document, ops, undo, .spark (SQLite), HTML   │
│  import, JSX/Tailwind/SVG export, tokens, query, ids      │
└───────────────────────────────────────────────────────────┘
        ▲ stdio                         ▲ HTTP + bearer token
   `spark mcp` relay  ◀──── agents (Claude Code, Codex, Cursor, …)
```

- **Cargo workspace** under `crates/`:
  - `spark-core`: pure library, no I/O except `.spark` file access. Fully unit-tested.
  - `spark-server`: axum server hosting the UI channel and the MCP endpoint over one shared document registry.
  - `spark-cli`: the `spark` binary. `spark serve` runs the server headless and serves the built web UI, so the UI also works in a plain browser for development and e2e tests. `spark mcp` is the stdio relay. `spark connect <agent>` writes agent configs. `spark export` exports from the command line.
- **`apps/desktop`**: Tauri v2 shell. It starts `spark-server` in-process and loads the web UI. It holds nothing the browser build can't do except native window, menus, deep links and file dialogs.
- **`web/`**: React + TypeScript + Tailwind v4, Vite, Bun. Radix UI primitives for menus, popovers, tooltips and dialogs. Lucide icons. Geist and Geist Mono fonts, bundled under their open license.
- **Fonts.** Designs can use installed fonts, the bundled Geist fonts and any Google Fonts family. The server downloads a Google Fonts family the first time the canvas needs it and keeps it in the config dir, so it works offline afterwards.
- **One UI transport.** The UI talks to Rust over a WebSocket in both the desktop app and the browser build, so Playwright can drive the real UI against `spark serve`.
- **Security.** The server binds `127.0.0.1` only. Every `/mcp` and `/ui` connection needs a bearer token generated on first run and stored in the user's Spark config dir with mode `0600`. Reject requests that carry an `Origin` header not belonging to the Spark UI, check `Host`, and answer `/.well-known/oauth-*` with a plain 404 so agents don't attempt OAuth.

## Document model

- **File**: ULID id, schema version (run migrations on load), ordered page list, design tokens, assets.
- **Page**: id `p-<base36>`, name, optional icon, canvas color. A **divider** is a page-list entry with no canvas.
- **Node** ids are `<base36 counter>-<site>`, for example `1EM-0`. The counter is file-wide; `site` is the local client number (0 for single-user), which keeps future multiplayer conflict-free.
- **Tree**: a flat node map plus one `(parent, sortKey)` entry per node, where sortKey is a fractional index. Top-level children of a page are **artboards**.
- **Node kinds** (v1): `Frame` (freeform or flex layout, optional clip), `Rect`, `Text` (single style per node in v1), `Svg` (with vector children), and image fills on Frame/Rect. Later: grid, rich text spans, pen paths, components and instances, shader fills.
- **Styles**: a typed subset of CSS for layout (size as fixed / percent / fill / hug per axis, position, flex direction, gap, padding, alignment, wrap) plus a structured design layer (fills: solid, linear/radial/conic gradient (with an interpolation color space), image; borders; radius; shadows; opacity; blend; blur and CSS filters; typography) that compiles to CSS. Keep a raw-CSS map for imported properties Spark doesn't model, and a set of disabled properties. Colors are stored as OKLCH and may reference tokens.
- **Design tokens** follow Tailwind v4 theme namespaces (`--color-*`, `--spacing-*`, `--radius-*`, `--font-*`, `--text-*`, `--font-weight-*`, `--tracking-*`, `--leading-*`, `--opacity-*`, `--breakpoint-*`, `--container-*`). Values may alias other tokens with `var(--x)`. Export as `@theme {}` or `:root {}`.
- **Edits** are a closed operation set: `Insert`, `Delete` (tombstone), `Move`, `Set { node, property, value }`, page ops, token ops, asset ops. Undo, persistence and the UI patch stream all use these ops.
- **Undo**: inverse-op transactions with selection and page, start/commit grouping for drags, a 250 ms merge window, and a 250-entry cap. Measured layout and other bookkeeping writes stay out of history. Agent edits are undoable as one transaction per tool call.

## Persistence: the `.spark` file

A single SQLite database: `meta`, `pages`, `nodes`, `tree`, `tokens`, `assets` (content-addressed blobs: hash, mime, bytes) and an `ops` log. Saves are incremental and atomic (WAL mode). Autosave runs continuously, so there is no Save button. `spark export --json` dumps a readable JSON tree for diffing.

Recent files and per-user settings live in the platform config dir, never inside a `.spark` file.

## Rendering

- Every node renders as a DOM element with inline CSS compiled from its style. The browser does layout and text.
- **Camera**: one container with a `matrix3d` transform for pan and zoom.
- **HUD**: a single 2D `<canvas>` above the content draws selection, hover outlines, resize handles, marquee, snap lines, measurement labels, artboard labels and agent activity. It redraws only when its state changes.
- **Measurement**: after layout, the UI batches measured rects (one message per frame) back to Rust. Rust keeps an R-tree for hit-testing, snapping and `find_nodes`, and reports sizes in MCP results. Nodes on inactive pages report unmeasured sizes as unknown.
- **Culling**: off-screen artboards skip rendering (`content-visibility: hidden` or unmount), and tiny nodes are skipped while zoomed far out.
- **Rasterizing** (screenshots, PNG/JPG/WebP export): Rust asks a UI connection to render a node to an image, with a call id and a timeout. Without a connected UI, raster exports fall back to Rust (SVG through resvg) and `get_screenshot` returns an error asking the user to open Spark. SVG export is generated in Rust, and PDF goes from that SVG through `krilla` on every platform.

## Editor UI

The layout matches a professional canvas editor, all in black and white:

- **App bar** (40 px): file tabs, home tab, window controls. Right-click a tab for Rename (inline), Copy link, Close, Close others, Close tabs to the right and Close all; drag tabs to reorder. Mod+1…8 jump to tab N, Mod+9 to the last tab, Ctrl+Tab and Ctrl+Shift+Tab cycle (Home included), Mod+T opens a new file, Mod+Shift+T reopens the last closed tab (kept for the session), Mod+Shift+D goes Home. Ctrl+Shift+? (also Help ▸ Keyboard Shortcuts) lists every shortcut in the keymap, grouped and searchable. A command can confirm itself with a short toast (`showToast`).
- **Left panel** (206 px including its 1 px border): a file header (the file name opens the main menu: rename, duplicate, keyboard shortcuts, settings, pixel grid / snap to pixel / layout guides toggles; a button hides the panel), a collapsible page list with dividers (current page checked, a dot on pages an agent is working on, drag rows to reorder, right-click a page to rename, duplicate, add a divider below, pick an icon or delete it), layer tree below (right-click a row to select it and open the same menu as the canvas; Mod+F searches layers by name: matches and their ancestors stay, Enter selects the first match, Esc closes). 28 px rows, 16 px indent per level, visibility and lock toggles on hover, rename on double-click, drag to reorder and reparent.
- **Chrome modes**: `.` or Mod+\ hides every panel and the toolbar; Shift+. floats them over the canvas as rounded cards. Toolbar buttons show a tooltip with the name and shortcut keycaps.
- **Toolbar** (41 px, vertical, between the left panel and the canvas): Move (V), Frame (F), Rectangle (R), Text (T), Image (I), Hand (H). Lucide icons, 36×32 buttons, the active tool shown as a filled black button with a white icon.
- **Canvas**: infinite plane with artboards and artboard name labels. Zoom from 2% to 25,600%. Space+drag or middle-drag to pan, pinch or Mod+wheel to zoom.
- **Right inspector** (280 px, 48 px header holding zoom and export): sections in order: Position and size, Layout (freeform or flex: direction, gap, padding, alignment, wrap), Appearance (opacity, radius, blend), Fill, Border, Shadow, Typography (text nodes only), Effects, Export. With nothing selected it shows page settings and the file's tokens. Also in the inspector: constraints (anchoring to the parent's edges, scaling or centring, for children of non-flex frames) and relative (%) sizing; flex reverse, baseline and align-content; per-side border widths; a separate Outline section; filters and shadow presets; a corner-radius slider; gradient tools (reverse, rotate 90°, distribute stops, color space); capture-from-layer image fills; text vertical alignment, wrap mode, truncation, decoration style and font features; a font picker (installed + Google Fonts, favorites, font tokens); eyedropper, token binding and detach on every color field; a selection-colors list that swaps a color everywhere in the selection; per-row export settings (JPEG quality, filename suffix) and Copy as PNG.
- **Zoom menu** (inspector header): zoom actions with shortcut keycaps, plus canvas preferences, also in Settings and stored on the device: invert zoom direction, scroll wheel zooms, right-click drag pans, zoom centers on selection, select the deepest layer by default (Mod flips it), reduce agent animations, and the nudge steps (default 1 px, 8 px with Shift).
- **Home**: recent files, new file, open file, drag a `.spark` file in. Archiving a recent file moves it (with its SQLite side files) into `Archive` in the files dir and off the list; deleting moves it to the system Trash. Either way, a file open in a tab, or with edits that failed to save, stays put.
- **Agent panel** (toggle Mod+J): shows connected agents and a log of their tool calls. It can also start a local agent CLI with Spark's MCP preconfigured and stream its output (see Agents).
- **Design tokens** live in a tokens view with editing, aliases and CSS export.

**Interactions**: click to select, Shift-click to add to the selection, Mod-click to deep-select, marquee to select many. Drag to move; Option-drag to duplicate; Escape during a drag, resize, rotate or handle drag puts everything back. Eight resize handles of 7 px; Shift keeps the aspect ratio, Option resizes from the center. Snapping to edges, centers and siblings within 4 px, with spacing guides. Arrow keys nudge 1 px, Shift+arrow 8 px. Double-click a text node to edit it inline. Enter selects children, Shift+Enter selects the parent. Right-click opens a context menu: select layer (the layers under the pointer, topmost first), copy, paste, paste here, paste on top (Mod+Shift+V), paste to replace (Mod+Shift+R), copy and paste styles (Mod+Alt+C / Mod+Alt+V), duplicate, delete, bring forward/back, group in frame (Mod+Alt+G), add flex layout (Shift+A), copy as HTML, JSX, Tailwind, PNG (Mod+Shift+C) or SVG, copy link, adjust text (font size, weight, line height and letter spacing steps), open image in new tab (image fills), export, hide, lock, rename. Right-clicking empty canvas opens a short menu instead: paste, paste here, select next or previous artboard, collapse all layers. Standard shortcuts: undo, redo, copy, paste, duplicate (Mod+D), select all, zoom to fit (Shift+1), zoom to selection (Shift+2), 100% (Mod+0), zoom in and out (Mod+= / Mod+-).

**Arrange and keyboard commands**: the Position section starts with align (left, horizontal center, right, top, vertical middle, bottom; Alt+A/H/D/W/V/S), distribute horizontally and vertically (3+ layers; Alt+Shift+H / Alt+Shift+V), rotate 90° clockwise, flip horizontal and vertical (Shift+H / Shift+V). One layer aligns within its parent; several align to their combined bounds. Layers in a flex parent are skipped. Tab / Shift+Tab select the next or previous sibling, N / Shift+N the next or previous artboard (zooming to it), Mod+Shift+A the other siblings, `\` the parent, Mod+Enter the children. Mod+Shift+G or Mod+Backspace ungroups selected frames into their children in place (one undo step); Mod+Alt+Up / Down move a layer one step earlier / later among its siblings. Aliases: A frame tool, Shift+F group in frame, Mod+Shift+K image. Zoom: 0 / Shift+0 for 100%, 1 for fit, 2 for selection, + / = zoom in, - zoom out. PageDown / PageUp switch pages, Alt+1 / Alt+2 switch the left panel between Design and Theme, Alt+L collapses the layer rows, Alt+C toggles clip content, Shift+X swaps fill and border colors. The canvas menu adds Ungroup, Select parent, Select children and an Align submenu.

**On-canvas aids**: Alt with a selection, hovering another node (or the selection's parent), draws distance lines with px badges between their edges. Hovering or selecting a flex frame hatches its padding and gap areas in gray, outlines its children with dashes and shows drag handles for each padding side and the gap (one undo step per drag). From 800% zoom a 1 px pixel grid shows (Shift+' or Mod+'); snap to pixel (Mod+Shift+', on by default) rounds drag and resize results to whole pixels, off keeps fractions. Dragging just outside a selected node's corner rotates it around its center (Shift snaps to 15°); the outline and size badge turn with it, and a turned node has no resize handles (use the inspector). Hold Z and drag to zoom to a rectangle, Z+click zooms in, Alt+Z+click zooms out. Double-click an artboard's name above it to rename it in place. The pixel grid, snap to pixel and layout guide visibility are per-browser view settings, not saved in the file.

**Layout guides**: a frame can carry column, row and grid guides (`layoutGuides` on the node: `kind`, `count`, `gutter`, `margin`, grid `size`). They are saved in the file, set through the `set` command (`prop: "layoutGuides"`, frames only, validated in core), edited in the inspector's Layout guides section, drawn as gray tints on the canvas (Shift+G toggles them) and never exported.

**Clipboard**: Spark's own format (lossless between Spark files); HTML with inline styles turns into nodes; SVG turns into SVG nodes; images become image fills; "copy as code" produces HTML, JSX or Tailwind. Copy styles keeps one layer's fills, border, radius, shadows, opacity, blend, blurs (and text type styles); paste styles applies them to the selection as one undo step. Paste on top puts the copied layers back at their original position inside the selection's parent; paste to replace swaps each selected layer for the copied ones at its position and parent (Spark clipboard content only). Copy as PNG writes an image to the system clipboard and copy as SVG writes the SVG text; both use the first selected layer.

**Theme**: light and dark, both pure grayscale. The token set is in `web/src/styles/tokens.css`, built from these values:

- Light: background `#FFFFFF`, panel `#FFFFFF`, canvas `#F2F2F2`, hairline `#E5E5E5`, hover `#F5F5F5`, selected `#000000` with white text, text `#000000` / `#555555` / `#8A8A8A`.
- Dark: background `#0A0A0A`, panel `#111111`, canvas `#1A1A1A`, hairline `#262626`, hover `#1C1C1C`, selected `#FFFFFF` with black text, text `#FFFFFF` / `#A3A3A3` / `#737373`.
- UI type: Geist 12 px (labels) and 13 px (titles), Geist Mono for values and code. Radius 6 px on controls, 8 px on popovers. Popover shadow `0 4px 16px rgb(0 0 0 / 0.12)`. Focus ring: 2 px solid currentColor with 1 px offset.
- Canvas HUD: selection and handles in black on light and white on dark, with a 1 px contrasting halo so they read over any design. Hover is a 1 px outline. Snap lines are dashed. Agent activity is shown with motion and texture (a falling-dot pattern above the artboard an agent is working on, a brief scan sweep over nodes it reads or writes), never with color.

## MCP server

**Transport**: MCP Streamable HTTP at `http://127.0.0.1:<port>/mcp` (default port `29890`, falling back to the next free port and writing the chosen port to the Spark config dir). Supports protocol versions `2024-11-05` through `2025-11-25`. A session is created on demand from any POST, so agents survive an app restart. `GET /mcp/config.json` returns the tool list and instructions without a session.

**Stdio relay**: `spark mcp` speaks stdio to the agent and forwards to the HTTP endpoint with the token. It answers `initialize` and `tools/list` itself, from the running server or a bundled snapshot. When Spark isn't running it returns a tool result telling the agent to ask the user to open Spark, and `spark mcp --headless` starts a headless server on demand.

**Routing**: file-scoped tools take a `fileId`: a ULID, a `/file/<id>/<pageId>` route, a `spark://` link, or a path to a `.spark` file. With no `fileId`, the file the user is viewing is used. If a targeted file is gone, fail instead of writing elsewhere. Page-scoped tools take an optional `pageId` and default to the viewed page. Every file-scoped result starts with a header `{file: {id, name, path}, contentHash: {tokens}}` (FNV-1a-32 of the token set) so agents notice when they've drifted to another file or tokens changed under them.

**Tools** (names chosen for familiarity with existing design-agent workflows; descriptions and guides are Spark's own words):

| Group | Tools |
|---|---|
| Files & pages | `list_resources`, `open_file`, `create_file`, `rename_resource`, `create_page`, `update_pages`, `arrange_pages` |
| Read | `get_basic_info`, `get_selection`, `get_node_info`, `get_children`, `get_tree_summary`, `get_jsx` (inline styles or Tailwind; optional node ids; optional root positioning), `get_computed_styles`, `find_nodes` (by text and/or style filters with wildcards and token-aware color matching), `get_screenshot` (scale 1 or 2, size-capped), `get_fill_image`, `get_font_family_info` (local system fonts plus Google Fonts metadata) |
| Write | `create_artboard` (auto-placed in empty space), `write_html` (`insert-children` or `replace`; supports `<spark-clone node-id>`, `layer-name`, local images via `spark-asset://` paths inside granted directories, `data:` URIs, http(s) image URLs, and `spark-gen://` generation URLs), `update_styles` (batched; reports ignored styles; `left`/`top` on an artboard set its canvas position), `set_text_content`, `rename_nodes`, `duplicate_nodes`, `move_nodes`, `delete_nodes`, `insert_image` (from path, URL or base64; returns the node id) |
| Tokens | `get_tokens` (json, css, tailwind), `create_tokens`, `set_tokens` |
| Images | `list_image_requests`, `fulfill_image_request` (see Images) |
| Export | `export` (png, jpg, webp, svg, pdf at a scale; saved under the user's Downloads or a given dir; returns paths), `export_combined_pdf` |
| Agent | `get_guide` (topics: `spark-mcp-instructions`, `image-generation`, `mobile-status-bar`), `finish_working_on_nodes` |
| Comments | later milestone |

Tool annotations must be honest: export, presence and file-writing tools are not `readOnlyHint`.

**Server instructions** point agents at the `spark-mcp-instructions` guide, then cover orienting with `get_basic_info` and `get_selection` (and `pageId` for other pages), building in small `write_html` batches checked with `get_screenshot`, content-sized heights, fonts from `get_font_family_info`, exact values for code from `get_jsx` and `get_computed_styles`, naming layers instead of showing ids, and `finish_working_on_nodes` at the end. The guide text is written fresh for Spark, versioned with the tools, and stored as markdown in the repo.

**Agent presence**: each MCP session is an agent with a name taken from `clientInfo` (Claude Code, Codex, Cursor, …). The agent panel lists connected agents, and with nothing selected the inspector lists the agents working in the file, grouped by page. The UI also shows a working indicator on tabs where an agent is active, the falling-dot pattern above artboards an agent is writing to, and a scan sweep on nodes it reads or writes. `finish_working_on_nodes` clears the indicators, and they also clear when the session ends.

## Agents

**One-click connect.** Home and the agent panel offer "Connect your agent". `spark connect <agent>` does the same from the CLI. Each writes an MCP entry named `spark` that runs `spark mcp` (or uses HTTP with the token header where the client supports it), editing config files in place:

| Agent | Config |
|---|---|
| Claude Code | `~/.claude.json` → `mcpServers.spark` |
| Codex | `~/.codex/config.toml` → `[mcp_servers.spark]` (format-preserving, `toml_edit`) |
| Cursor | `~/.cursor/mcp.json` → `mcpServers.spark` |
| VS Code / Copilot | user `mcp.json` → `servers.spark`; `~/.copilot/mcp-config.json` |
| OpenCode | `~/.config/opencode/opencode.json(c)` → `mcp.spark` |
| Gemini CLI | `~/.gemini/settings.json` → `mcpServers.spark` |
| Antigravity | `~/.gemini/config/mcp_config.json` (or the older `~/.gemini/antigravity/mcp_config.json` when only that exists) → `mcpServers.spark` |
| Claude Desktop | generated `.mcpb` bundle opened with the system handler |

Rules for every writer: idempotent, comment- and formatting-preserving (JSONC and TOML), atomic write, keep the existing file mode and use `0600` for new files, refuse to touch a file that doesn't parse, and print exactly what changed. `spark connect --dry-run` shows the diff. `spark disconnect <agent>` removes the entry.

**Direct agent panel.** Inside Spark the user can pick an installed agent CLI (detected on `PATH`: `claude`, `codex`, `opencode`, `gemini`, `cursor-agent`), type a prompt, and Spark runs it headless in the file's context with Spark's MCP already attached (for example `claude -p --mcp-config <generated>`), streaming the agent's output into the panel while its edits appear on the canvas. Spark never stores the agents' credentials; each CLI uses its own login.

## Images

1. **Agent-provided.** Any agent with its own image tools (or none) can place images through `insert_image`, `write_html` with a `data:` or http(s) URL, or a local file path. Images are stored content-addressed inside the `.spark` file.
2. **Image requests (works with every agent).** `spark-gen://agent?prompt=...&aspect_ratio=16:9` creates a placeholder node with an `imageRequest {status: pending, prompt, aspectRatio, referenceNodes}`. `list_image_requests` returns pending requests, and `fulfill_image_request {requestId, path | url | base64}` fills them. This lets any connected agent, using whatever image model it has access to, generate the image.
3. **Built-in providers, bring your own key.** `spark-gen://<provider>/<model>?prompt=...&aspect_ratio=...&reference_nodes=...` runs asynchronously in Rust. The node shows a generating state and agents poll `get_node_info.imageGeneration.status` (`processing`, `ready`, `error`). A job still `processing` when its file is closed or Spark quits becomes `error` the next time the file opens. Providers are a Rust trait with adapters for OpenAI Images, Google Gemini images, fal.ai, Replicate, OpenRouter, and local ComfyUI or Automatic1111 endpoints. Keys are entered in Settings and stored in the OS keychain (`keyring` crate), never in `.spark` files, the repo, or logs.
4. **Cost guard.** Generation only runs for providers the user enabled. A per-call cap (default 10 images) and an optional monthly budget reject a whole batch when it would go over. The guide tells agents to generate only when the user asked for imagery.

## Export

PNG, JPG, WebP (1×–4× or a target width or height), SVG, PDF (one node, or many nodes combined one per page), HTML, JSX with inline styles or Tailwind classes, and tokens as CSS. The export section in the inspector holds per-node export presets. All export also works from `spark export <file> --node <id> --format png` and through MCP.

## Desktop shell

Tauri v2 with `tauri-plugin-deep-link` (`spark://file/<id>/<pageId>?node=<id>`), `tauri-plugin-single-instance` (forwards links and opened files to the running instance), native menus with standard shortcuts, `.spark` file association, multiple tabs, background throttling disabled for documents agents are working on, and window state restore. Signed builds and auto-update are not part of v1.

## Testing

- `cargo test` covers spark-core (ops, undo, ids, fractional index, persistence round-trip, HTML import, JSX export, token parsing) and spark-server (MCP protocol, routing, auth).
- Vitest covers UI logic that isn't drawing (geometry, snapping, keymaps).
- **End-to-end (Playwright)** runs the real web UI in Chromium and WebKit against `spark serve` on a temp data dir. Required flows:
  1. Create a file, draw a frame and text, edit in the inspector, undo and redo, reload, and see the state persisted.
  2. Connect an MCP client (the official TypeScript SDK) with the token, `write_html` an artboard, and see it appear in the DOM. `get_jsx` round-trips, and `get_screenshot` returns a PNG.
  3. The image request flow: `spark-gen://agent` placeholder → `fulfill_image_request` → image visible.
  4. `spark connect --dry-run` for each agent against fixture config files.
  5. Performance smoke: load a generated 20k-node file and assert the open-time budget.
- `bun run verify` runs everything: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, `tsc --noEmit`, lint, vitest, a web build, and Playwright e2e.

## Out of scope for v1

Accounts, cloud sync, multiplayer, comments, components and instances, pen tool, shader fills, video export, Figma import, plugins, and auto-update. The model (site-tagged ids, an op log, tombstones) is designed so multiplayer and components can be added later without a format break.
