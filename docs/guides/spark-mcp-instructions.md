# Working in Spark

Spark is a design canvas built from real HTML and CSS. Every layer is a DOM element with inline styles, so what you write is what renders, and what renders exports as code. This guide explains how to read and edit a Spark file well.

## The model

- A **file** holds **pages**. Each page is an infinite canvas. Top-level frames on a page are **artboards** (screens, slides, components).
- Layers are **frames** (boxes that hold children, laid out freeform or with flexbox), **rectangles**, **text** (one style per text layer), and **SVG** (with vector children). Images are image fills on frames and rectangles.
- Node ids look like `12-0`. Page ids look like `p-1`. Use them in tool calls, but never show them to the user; refer to layers by name.
- Every file-scoped result starts with a header naming the file and a hash of its design tokens. If the file is not the one you meant, stop and re-target with `fileId`. If the token hash changed since you last read tokens, read them again.

## Start of a session

1. Call `get_basic_info` to see pages, artboards, fonts in use and tokens.
2. Call `get_selection` to see what the user is pointing at. "This", "here" and "it" usually mean the selection.
3. Pass `pageId` explicitly when you work on a page other than the one the user is viewing.

## Writing designs

- Create a screen with `create_artboard` (it is placed in free space), then fill it with `write_html`.
- Build in small steps: send each section (a header, then a hero, then a row of cards) as its own `write_html` call. Small calls show progress live, are easier to fix, and undo cleanly (each tool call is one undo step).
- Use inline `style` attributes. Class names and `<style>` blocks are ignored.
- Lay out with flexbox (`display: flex`, `gap`, `padding`, `align-items`, `justify-content`). Prefer `width: 100%` or flex sizing over fixed pixel widths inside containers.
- Let content set the height: leave `height` unset or use `height: fit-content` instead of a pixel value, then confirm with a screenshot.
- `layer-name="Pricing card"` names a layer. Name the important groups; it helps the user and you.
- Reuse an existing layer with `<spark-clone node-id="…"></spark-clone>`; inline styles on the clone element override the copy's own.
- Text inside an element that holds only text and inline tags (`b`, `em`, `span`, `a`, `br`) becomes one text layer.
- Images: `<img src>` accepts `data:` URIs, http(s) URLs, and local files as `spark-asset:///absolute/path.png` when the folder is granted in Spark's settings. `spark-gen://agent?prompt=…` makes a placeholder you can fill later; see the `image-generation` guide.
- Use `update_styles` for small changes, `set_text_content` for copy edits, and `move_nodes`, `duplicate_nodes`, `delete_nodes` and `rename_nodes` for structure. To arrange artboards on the canvas, set their `left` and `top` with `update_styles`; `move_nodes` changes order and parents, not canvas position.

## Typography

Call `get_font_family_info` before you pick fonts. Geist and Geist Mono always work. Use fonts that are installed or available from Google Fonts, and stick to the weights the family provides.

## Checking your work

- Call `get_screenshot` on the artboard after each meaningful change. Look for clipped text, overflow, uneven spacing, and low contrast.
- `get_tree_summary` shows the structure compactly; `get_node_info` shows one layer's size and position. Sizes are in CSS pixels.
- For code export, take exact numbers and colors from `get_jsx` and `get_computed_styles`; a screenshot only shows how it looks.

## Tokens

Design tokens are CSS custom properties in Tailwind v4 theme namespaces (`--color-*`, `--spacing-*`, `--radius-*`, `--font-*`, `--text-*` and so on). Use them in styles as `var(--color-brand)`. Create them with `create_tokens`, change or remove them with `set_tokens`, and read them with `get_tokens`.

## Finishing

When you are done, call `finish_working_on_nodes` so the activity indicators on the user's canvas clear. Then tell the user what you changed, by layer name.
