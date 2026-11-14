//! The MCP tool catalog: names, descriptions, JSON Schemas and honest annotations.

use serde_json::{Value, json};

const FILE_ID: &str = "File to work in: a file id, a /file/<id>/<pageId> route, a spark:// link or a path to a .spark file. Defaults to the file the user is viewing.";
const PAGE_ID: &str = "Page id (p-…). Defaults to the page the user is viewing.";

fn file() -> Value {
    json!({"type": "string", "description": FILE_ID})
}

fn page() -> Value {
    json!({"type": "string", "description": PAGE_ID})
}

fn ids(desc: &str) -> Value {
    json!({"type": "array", "items": {"type": "string"}, "minItems": 1, "description": desc})
}

#[derive(Clone, Copy)]
enum Kind {
    /// Reads the document only.
    Read,
    /// Changes the document or files, but nothing is lost (undoable or additive).
    Write,
    /// Removes content.
    Destructive,
}

fn tool(name: &str, kind: Kind, description: &str, properties: Value, required: &[&str]) -> Value {
    let annotations = match kind {
        Kind::Read => {
            json!({"readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false})
        }
        Kind::Write => json!({"readOnlyHint": false, "destructiveHint": false, "openWorldHint": false}),
        Kind::Destructive => json!({"readOnlyHint": false, "destructiveHint": true, "openWorldHint": false}),
    };
    json!({
        "name": name,
        "description": description,
        "inputSchema": {"type": "object", "properties": properties, "required": required, "additionalProperties": false},
        "annotations": annotations,
    })
}

fn themes() -> Value {
    json!({"type": "object", "description": "Values per theme name (lowercase words joined by -, e.g. dark).", "additionalProperties": {"type": ["string", "number", "null"]}})
}

pub fn catalog() -> Vec<Value> {
    use Kind::*;
    vec![
        // Files and pages
        tool(
            "list_resources",
            Read,
            "List Spark files: open files first, then recently opened ones, with ids, names and paths.",
            json!({"limit": {"type": "integer", "minimum": 1, "maximum": 200, "default": 50}}),
            &[],
        ),
        tool(
            "open_file",
            Write,
            "Open a file in Spark (in a background tab if it isn't open) and return its basic info. pageId only applies when the file opens fresh.",
            json!({"fileId": {"type": "string", "description": FILE_ID}, "pageId": page()}),
            &["fileId"],
        ),
        tool(
            "create_file",
            Write,
            "Create a new Spark file, optionally as a copy of another file. Returns the new file's id, name and path.",
            json!({"name": {"type": "string"}, "cloneFileId": {"type": "string", "description": "File to copy."}}),
            &[],
        ),
        tool(
            "rename_resource",
            Write,
            "Rename a file.",
            json!({"resourceId": {"type": "string", "description": "File id."}, "name": {"type": "string", "minLength": 1, "maxLength": 500}}),
            &["resourceId", "name"],
        ),
        tool(
            "create_page",
            Write,
            "Add a page after the page the user is viewing. Returns the new page id.",
            json!({"fileId": file(), "name": {"type": "string"}, "icon": {"type": "string", "enum": spark_core::node::PAGE_ICONS}}),
            &[],
        ),
        tool(
            "update_pages",
            Write,
            "Rename pages or change their icons, in one batch, without switching the user's page.",
            json!({"fileId": file(), "updates": {"type": "array", "minItems": 1, "items": {"type": "object", "properties": {"pageId": {"type": "string"}, "name": {"type": "string"}, "icon": {"type": ["string", "null"], "enum": [null, "arrow", "padlock", "check", "cross", "info", "question", "design", "vector", "images", "table", "presentation", "blueprint", "swatches", "bar-chart", "line-chart", "pie-chart", "page"]}}, "required": ["pageId"], "additionalProperties": false}}}),
            &["updates"],
        ),
        tool(
            "arrange_pages",
            Write,
            "Set the full page order, adding dividers with {\"type\":\"divider\"} entries and removing dividers by id. The order must list every page.",
            json!({"fileId": file(), "order": {"type": "array", "items": {"anyOf": [{"type": "string"}, {"type": "object", "properties": {"type": {"const": "divider"}}, "required": ["type"]}]}}, "removeDividerIds": {"type": "array", "items": {"type": "string"}}}),
            &["order"],
        ),
        // Read
        tool(
            "get_basic_info",
            Read,
            "File name, pages, the viewed page's artboards with sizes and positions, node count, fonts in use and tokens. Call this first.",
            json!({"fileId": file(), "pageId": page()}),
            &[],
        ),
        tool(
            "get_selection",
            Read,
            "The layers the user has selected, with names, kinds and sizes.",
            json!({"fileId": file()}),
            &[],
        ),
        tool(
            "get_node_info",
            Read,
            "Details for one layer: kind, name, size and position (world and relative to the parent), parent, children, text, visibility, lock, and image generation status.",
            json!({"fileId": file(), "nodeId": {"type": "string"}}),
            &["nodeId"],
        ),
        tool(
            "get_children",
            Read,
            "The direct children of a layer or page, in order.",
            json!({"fileId": file(), "nodeId": {"type": "string", "description": "Layer id or page id."}}),
            &["nodeId"],
        ),
        tool(
            "get_tree_summary",
            Read,
            "An indented outline of a subtree: kind, name, id and size per line. Sizes are ? when not measured yet.",
            json!({"fileId": file(), "nodeId": {"type": "string"}, "depth": {"type": "integer", "minimum": 1, "maximum": 10, "default": 3}}),
            &["nodeId"],
        ),
        tool(
            "get_jsx",
            Read,
            "React JSX for a subtree, with inline styles or Tailwind classes. Exact values for code come from here.",
            json!({"fileId": file(), "nodeId": {"type": "string"}, "nodeIds": {"type": "array", "items": {"type": "string"}}, "format": {"type": "string", "enum": ["inline-styles", "tailwind"], "default": "inline-styles"}, "includeNodeIds": {"type": "boolean", "default": true}, "includeRootPositioning": {"type": "boolean", "default": false}}),
            &[],
        ),
        tool(
            "get_computed_styles",
            Read,
            "The CSS each layer renders with, as camelCase properties, for several layers at once.",
            json!({"fileId": file(), "nodeIds": ids("Layers to read.")}),
            &["nodeIds"],
        ),
        tool(
            "find_nodes",
            Read,
            "Find layers by text and/or CSS. Text matches case-insensitively; * is a wildcard. Style filters are ANDed; colors match by value, so #fff finds white written any way, including through tokens.",
            json!({"fileId": file(), "pageId": page(), "nodeId": {"type": "string", "description": "Search only inside this layer."}, "textValue": {"type": "string"}, "filters": {"type": "array", "items": {"type": "object", "properties": {"styleName": {"type": "string"}, "styleValue": {"type": "string"}}, "additionalProperties": false}}, "limit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 100}}),
            &[],
        ),
        tool(
            "get_screenshot",
            Read,
            "A PNG of a layer as it renders on the canvas. Use scale 2 for detail. Needs the Spark window open.",
            json!({"fileId": file(), "nodeId": {"type": "string"}, "scale": {"type": "number", "enum": [1, 2], "default": 1}}),
            &["nodeId"],
        ),
        tool(
            "get_fill_image",
            Read,
            "The image fill of a layer as a JPEG sized for viewing, with its original size and type.",
            json!({"fileId": file(), "nodeId": {"type": "string"}}),
            &["nodeId"],
        ),
        tool(
            "get_font_family_info",
            Read,
            "Whether font families are available (installed locally, bundled with Spark, or on Google Fonts) and which weights and styles each has. Call before choosing fonts.",
            json!({"fileId": file(), "familyNames": {"type": "array", "items": {"type": "string"}, "minItems": 1}}),
            &["familyNames"],
        ),
        // Write
        tool(
            "create_artboard",
            Write,
            "Create a top-level frame (an artboard) in free space on a page. Styles are camelCase CSS; width defaults to 1440px and height to fit-content. Returns the artboard id.",
            json!({"fileId": file(), "pageId": page(), "name": {"type": "string"}, "styles": {"type": "object", "additionalProperties": {"type": ["string", "number"]}}}),
            &["name"],
        ),
        tool(
            "write_html",
            Write,
            "Turn HTML with inline styles into layers. mode insert-children appends inside targetNodeId (a layer or page); replace swaps targetNodeId for the new layers. Supports layer-name, <spark-clone node-id>, data: and http(s) images, spark-asset:// paths in granted folders and spark-gen:// image requests. Write one visual group per call.",
            json!({"fileId": file(), "targetNodeId": {"type": "string"}, "html": {"type": "string"}, "mode": {"type": "string", "enum": ["insert-children", "replace"], "default": "insert-children"}}),
            &["targetNodeId", "html"],
        ),
        tool(
            "update_styles",
            Write,
            "Change CSS on layers, in batches. Styles are camelCase CSS (null removes a property). On an artboard, left and top set its position on the canvas, so use them to arrange artboards. `theme` makes a frame and its contents use a token theme (null for the base values). Reports any styles that were ignored.",
            json!({"fileId": file(), "updates": {"type": "array", "minItems": 1, "items": {"type": "object", "properties": {"nodeIds": ids("Layers to change."), "styles": {"type": "object", "additionalProperties": {"type": ["string", "number", "null"]}}, "theme": {"type": ["string", "null"], "description": "Token theme name, e.g. dark."}}, "required": ["nodeIds"], "additionalProperties": false}}}),
            &["updates"],
        ),
        tool(
            "set_text_content",
            Write,
            "Replace the text of text layers, in one batch.",
            json!({"fileId": file(), "updates": {"type": "array", "minItems": 1, "items": {"type": "object", "properties": {"nodeId": {"type": "string"}, "textContent": {"type": "string"}}, "required": ["nodeId", "textContent"], "additionalProperties": false}}}),
            &["updates"],
        ),
        tool(
            "rename_nodes",
            Write,
            "Rename layers, in one batch.",
            json!({"fileId": file(), "updates": {"type": "array", "minItems": 1, "items": {"type": "object", "properties": {"nodeId": {"type": "string"}, "name": {"type": "string", "minLength": 1}}, "required": ["nodeId", "name"], "additionalProperties": false}}}),
            &["updates"],
        ),
        tool(
            "duplicate_nodes",
            Write,
            "Deep-copy layers, next to the originals or into parentId. Copied artboards go to free space. Returns new ids and a map from old descendant ids to new ones.",
            json!({"fileId": file(), "nodes": {"type": "array", "minItems": 1, "items": {"type": "object", "properties": {"id": {"type": "string"}, "parentId": {"type": "string"}}, "required": ["id"], "additionalProperties": false}}}),
            &["nodes"],
        ),
        tool(
            "move_nodes",
            Write,
            "Reorder or reparent layers, keeping their ids. Each move gives before, after, or parentId (a layer, a page, or 'root' for the viewed page) with an optional index. Moves apply in order.",
            json!({"fileId": file(), "moves": {"type": "array", "minItems": 1, "items": {"type": "object", "properties": {"nodeId": {"type": "string"}, "before": {"type": "string"}, "after": {"type": "string"}, "parentId": {"type": "string"}, "index": {"type": "integer", "minimum": 0}}, "required": ["nodeId"], "additionalProperties": false}}}),
            &["moves"],
        ),
        tool(
            "delete_nodes",
            Destructive,
            "Delete layers and everything inside them. The user can undo it.",
            json!({"fileId": file(), "nodeIds": ids("Layers to delete.")}),
            &["nodeIds"],
        ),
        tool(
            "insert_image",
            Write,
            "Place an image as a new layer with an image fill. Give one of path (inside a granted folder), url or base64. Returns the new layer id.",
            json!({"fileId": file(), "parentId": {"type": "string", "description": "Layer or page to add it to. Defaults to the viewed page."}, "path": {"type": "string"}, "url": {"type": "string"}, "base64": {"type": "string"}, "name": {"type": "string"}, "width": {"type": "number"}, "height": {"type": "number"}, "x": {"type": "number"}, "y": {"type": "number"}}),
            &[],
        ),
        // Tokens
        tool(
            "get_tokens",
            Read,
            "List design tokens as JSON, or as CSS (:root) or Tailwind (@theme).",
            json!({"fileId": file(), "types": {"type": "array", "items": {"type": "string", "enum": ["color", "spacing", "radius", "fontFamily", "fontSize", "fontWeight", "letterSpacing", "lineHeight", "opacity", "breakpoint", "container"]}}, "namePattern": {"type": "string", "description": "Glob on the name, e.g. --color-brand-*."}, "format": {"type": "string", "enum": ["json", "css", "tailwind"], "default": "json"}}),
            &[],
        ),
        tool(
            "create_tokens",
            Write,
            "Create design tokens. Names are CSS custom properties in Tailwind theme namespaces (--color-*, --spacing-*, …). Values may alias other tokens with var(--name). `themes` gives values for named themes such as dark; layers with that theme (update_styles `theme`, or data-theme in write_html) use them.",
            json!({"fileId": file(), "tokens": {"type": "array", "minItems": 1, "items": {"type": "object", "properties": {"type": {"type": "string"}, "name": {"type": "string", "pattern": "^--[\\w-]+$"}, "value": {"type": ["string", "number"]}, "description": {"type": "string", "maxLength": 1024}, "themes": themes()}, "required": ["name", "value"], "additionalProperties": false}}}),
            &["tokens"],
        ),
        tool(
            "set_tokens",
            Destructive,
            "Rename, change or delete design tokens, or set their values for named themes (null removes a theme value).",
            json!({"fileId": file(), "tokens": {"type": "array", "minItems": 1, "items": {"type": "object", "properties": {"name": {"type": "string"}, "newName": {"type": "string"}, "value": {"type": ["string", "number"]}, "description": {"type": "string"}, "themes": themes(), "delete": {"type": "boolean"}}, "required": ["name"], "additionalProperties": false}}}),
            &["tokens"],
        ),
        // Agent
        tool(
            "export",
            Write,
            "Export layers as image or document files (png, jpg, webp, svg, pdf). Give per-layer settings, or \"nodes-with-exports-only\" to use the export presets saved on layers. Files go to the user's Downloads folder (or dir) without overwriting; returns their paths.",
            json!({"fileId": file(), "pageId": page(), "nodes": {"anyOf": [{"const": "nodes-with-exports-only"}, {"type": "object", "additionalProperties": {"type": "array", "items": {"type": "object", "properties": {"format": {"type": "string", "enum": ["png", "jpg", "webp", "svg", "pdf"]}, "scale": {"type": "string", "pattern": "^\\d+(\\.\\d+)?[xwh]?$", "default": "1x"}}, "required": ["format"], "additionalProperties": false}}}]}, "dir": {"type": "string", "description": "Folder to write to. Defaults to Downloads."}}),
            &["nodes"],
        ),
        tool(
            "export_combined_pdf",
            Write,
            "Export several layers as one PDF, one page per layer, ordered top to bottom then left to right. Returns the file path.",
            json!({"fileId": file(), "nodeIds": ids("Layers to include."), "name": {"type": "string"}, "dir": {"type": "string"}}),
            &["nodeIds"],
        ),
        tool(
            "list_image_requests",
            Read,
            "Image placeholders waiting for an image (made with spark-gen://agent URLs), with prompts, aspect ratios and reference layers.",
            json!({"fileId": file()}),
            &[],
        ),
        tool(
            "fulfill_image_request",
            Write,
            "Fill an image request with an image you made. Give one of path (inside a granted folder), url or base64.",
            json!({"fileId": file(), "requestId": {"type": "string"}, "path": {"type": "string"}, "url": {"type": "string"}, "base64": {"type": "string"}}),
            &["requestId"],
        ),
        tool(
            "get_guide",
            Read,
            "Read a Spark guide: spark-mcp-instructions (read first), image-generation, or mobile-status-bar.",
            json!({"topic": {"type": "string", "enum": ["spark-mcp-instructions", "image-generation", "mobile-status-bar"]}}),
            &["topic"],
        ),
        tool(
            "finish_working_on_nodes",
            Write,
            "Clear your activity indicators on the canvas, for all layers or just the given ones. Call when you're done.",
            json!({"fileId": file(), "nodeIds": {"type": "array", "items": {"type": "string"}}}),
            &[],
        ),
    ]
}

pub fn guide(topic: &str) -> Option<&'static str> {
    match topic {
        "spark-mcp-instructions" => Some(include_str!("../../../../docs/guides/spark-mcp-instructions.md")),
        "image-generation" => Some(include_str!("../../../../docs/guides/image-generation.md")),
        "mobile-status-bar" => Some(include_str!("../../../../docs/guides/mobile-status-bar.md")),
        _ => None,
    }
}

pub const INSTRUCTIONS: &str = "Spark files are pages of artboards, and every layer is real HTML and CSS.\n\
Read get_guide({topic: \"spark-mcp-instructions\"}) once before you edit; it covers layout, fonts, images and tokens.\n\
Orient with get_basic_info (pages, artboards, tokens) and get_selection (what the user picked). Page-scoped calls use the page on screen unless you pass pageId.\n\
Build a design in small write_html batches and look at each one with get_screenshot before moving on. Let containers grow with their content instead of setting heights you would have to guess.\n\
Choose typefaces after get_font_family_info shows what is installed or downloadable.\n\
When turning a design into code, take numbers from get_jsx and get_computed_styles; screenshots are for looking, not measuring.\n\
Talk about layers by name; ids are for tool calls.\n\
Call finish_working_on_nodes when you stop, to clear your activity marks.";
