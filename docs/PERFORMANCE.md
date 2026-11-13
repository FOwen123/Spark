# Performance

Spark's spec sets four budgets. This page records how they are measured and the latest results.

| Target (spec) | Result | How it's measured |
|---|---|---|
| Open a 20,000-node file in under 1 s | **Core: 33 ms** to load the `.spark` file (release). **App: about 0.4 s** from navigation to the artboards and layers on screen. | `crates/spark-core/tests/bench.rs`; `e2e/perf.spec.ts` (asserts < 1 s in Chromium and WebKit on every local `bun run verify`; < 2 s on GitHub's slower CI Macs) |
| Apply an edit op in under 1 ms | **3.2 µs** per `Set` op through the editor, including undo bookkeeping (release) | `crates/spark-core/tests/bench.rs` (asserts < 1 ms) |
| MCP read tools under 10 ms p50 (excluding screenshots) | **0.3–2.1 ms p50** over HTTP on a 20k-node file (release); slowest is `get_basic_info` | `crates/spark-server/tests/mcp_perf.rs` (asserts < 10 ms) |
| Pan and zoom at 60 fps on a 5,000-node page | **60 fps average, p95 16.8 ms**, worst frame 17–67 ms depending on machine load; up to 5,096 nodes in the DOM | `e2e/panzoom.bench.ts` (Chromium) |

Recorded on 2026-10-22 (pan and zoom row: 2026-10-29) on an Apple M5 Pro Mac running macOS 26.6, with the server built in release mode for the Rust rows and in the optimized dev profile for the app row. Run everything with `bun run bench` (the pan/zoom run is opt-in because frame timing depends on the machine). The 20k-node file is generated with `spark demo <path> --nodes 20000`; it is synthetic demo content.

## MCP read latency (release, 20,090 nodes, 30 calls each)

| Tool | p50 | p90 |
|---|---|---|
| get_basic_info | 2.05 ms | 2.96 ms |
| get_selection | 0.29 ms | 0.32 ms |
| get_node_info | 0.31 ms | 0.33 ms |
| get_children | 0.41 ms | 0.42 ms |
| get_tree_summary (depth 3) | 0.41 ms | 0.43 ms |
| get_jsx | 0.37 ms | 0.39 ms |
| get_computed_styles | 0.41 ms | 0.44 ms |
| find_nodes | 1.23 ms | 2.17 ms |
| get_tokens | 0.42 ms | 0.53 ms |

## What makes it fast

- **SQLite, one row per node.** Opening reads rows and builds the tree index; saving writes only what changed, in one WAL transaction.
- **Level of detail on the canvas.** Snapshots carry each node's last layout, so the canvas places artboards before it measures anything. An artboard mounts its contents when it is on or near the screen and at least 32 px across, so zoomed-out pages still show previews. Artboards nearest the view centre mount first, one per frame, and only while the mounted layers stay under 6,000; the rest render as sized shells. A 20k-node file therefore opens with about 6,000 elements in the DOM.
- **Per-node subscriptions.** The UI mirror notifies only the components whose node changed, so a patch re-renders a handful of elements.
- **A compositor-only camera.** Pan and zoom set one `matrix3d` transform. Wheel, pinch and hand drags move the camera outside React, easing toward the target each frame, and commit it to React only after the gesture pauses; the HUD canvas redraws only when its inputs change.
- **Headless layout is cached.** When nothing has measured a page (files made by agents or the CLI), Spark lays it out with taffy using cached glyph advances, and saves the result with the file.

## Known limits

- The first open of a very large file that has never been laid out spends time on headless layout (about 0.4 s for 20k nodes in a release build). Later opens reuse the saved layout.
