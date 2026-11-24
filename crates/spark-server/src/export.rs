//! File export: PNG, JPG and WebP (rendered by a connected UI when there is one, else from Rust
//! SVG with resvg), SVG from Rust, PDF from that SVG via krilla, and code (HTML, JSX, Tailwind).
//! Files land in the user's Downloads (or a given folder) without overwriting anything.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

use resvg::usvg::{self, fontdb};
use serde::Deserialize;
use spark_core::{Document, NodeId, codegen, svg};

use crate::hub::Hub;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Png,
    Jpg,
    Webp,
    Svg,
    Pdf,
    Html,
    Jsx,
    Tailwind,
}

impl Format {
    pub fn parse(s: &str) -> Result<Format, String> {
        Ok(match s.to_ascii_lowercase().as_str() {
            "png" => Format::Png,
            "jpg" | "jpeg" => Format::Jpg,
            "webp" => Format::Webp,
            "svg" => Format::Svg,
            "pdf" => Format::Pdf,
            "html" => Format::Html,
            "jsx" => Format::Jsx,
            "tailwind" => Format::Tailwind,
            other => {
                return Err(format!("unknown export format `{other}` (png, jpg, webp, svg, pdf, html, jsx, tailwind)"));
            }
        })
    }

    pub fn ext(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpg => "jpg",
            Format::Webp => "webp",
            Format::Svg => "svg",
            Format::Pdf => "pdf",
            Format::Html => "html",
            Format::Jsx | Format::Tailwind => "jsx",
        }
    }

    pub fn is_raster(self) -> bool {
        matches!(self, Format::Png | Format::Jpg | Format::Webp)
    }
}

/// `2x`, `0.5x`, `512w` (target width) or `512h` (target height).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scale {
    Factor(f64),
    Width(f64),
    Height(f64),
}

impl Scale {
    pub fn parse(s: &str) -> Result<Scale, String> {
        let t = s.trim().to_ascii_lowercase();
        let n = |v: &str| {
            v.parse::<f64>()
                .ok()
                .filter(|v| *v > 0.0 && v.is_finite())
                .ok_or_else(|| format!("bad export scale `{s}` (use 2x, 512w or 512h)"))
        };
        Ok(match t.chars().last() {
            Some('x') => Scale::Factor(n(&t[..t.len() - 1])?),
            Some('w') => Scale::Width(n(&t[..t.len() - 1])?),
            Some('h') => Scale::Height(n(&t[..t.len() - 1])?),
            _ => Scale::Factor(n(&t)?),
        })
    }

    /// The pixel scale for a node of size (w, h), capped at 4×.
    pub fn factor(self, w: f64, h: f64) -> f64 {
        let f = match self {
            Scale::Factor(f) => f,
            Scale::Width(px) => px / w.max(1.0),
            Scale::Height(px) => px / h.max(1.0),
        };
        f.clamp(0.01, 4.0)
    }

    pub fn suffix(self) -> String {
        match self {
            Scale::Factor(1.0) => String::new(),
            Scale::Factor(f) => format!("@{}x", spark_core::style::Len::Num(f).to_string().trim_end_matches(".0")),
            Scale::Width(w) => format!("@{w}w"),
            Scale::Height(h) => format!("@{h}h"),
        }
    }
}

// ---- fonts and text measurement --------------------------------------------------------------

const BUNDLED: &[&[u8]] = &[
    include_bytes!("../fonts/Geist-Regular.ttf"),
    include_bytes!("../fonts/Geist-Medium.ttf"),
    include_bytes!("../fonts/Geist-SemiBold.ttf"),
    include_bytes!("../fonts/Geist-Bold.ttf"),
    include_bytes!("../fonts/GeistMono-Regular.ttf"),
    include_bytes!("../fonts/GeistMono-Bold.ttf"),
];

/// Spark's config dir, whose `fonts/files` holds the Google Fonts the canvas downloaded.
static CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();
/// Bumped when a font is downloaded, so the next export picks it up.
static FONTS_GEN: AtomicU64 = AtomicU64::new(0);

/// Marks the downloaded fonts as changed.
pub fn fonts_changed() {
    FONTS_GEN.fetch_add(1, Ordering::Relaxed);
}

/// System fonts, Spark's bundled Geist and downloaded Google Fonts, shared by measurement, SVG
/// raster and PDF.
pub fn fonts() -> Arc<fontdb::Database> {
    struct Loaded {
        generation: u64,
        seen: HashSet<PathBuf>,
        db: Arc<fontdb::Database>,
    }
    static DB: Mutex<Option<Loaded>> = Mutex::new(None);
    let generation = FONTS_GEN.load(Ordering::Relaxed);
    let mut lock = DB.lock().unwrap();
    let loaded = lock.get_or_insert_with(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        for f in BUNDLED {
            db.load_font_data(f.to_vec());
        }
        db.set_sans_serif_family("Geist");
        db.set_monospace_family("Geist Mono");
        Loaded { generation: u64::MAX, seen: HashSet::new(), db: Arc::new(db) }
    });
    if loaded.generation != generation {
        loaded.generation = generation;
        if let Some(dir) = CONFIG_DIR.get() {
            load_downloaded(&dir.join("fonts"), &mut loaded.seen, &mut loaded.db);
        }
    }
    loaded.db.clone()
}

/// Adds the downloaded Google Fonts in `dir` (Spark's `fonts` cache) not in `seen`, named as their
/// cached `@font-face` rules name them, as the browser does: a file's own family name can differ
/// ("DM Sans 9pt"). Google splits a family into Unicode subsets; only the Latin one has the whole
/// a-z, and loading just that keeps font matching on the subset that covers most text.
// shortcut: other subsets (Cyrillic, CJK slices) fall back to installed fonts; load them per character if designs need it.
fn load_downloaded(dir: &Path, seen: &mut HashSet<PathBuf>, db: &mut Arc<fontdb::Database>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if e.path().extension().is_none_or(|x| x != "css") {
            continue;
        }
        let Ok(css) = std::fs::read_to_string(e.path()) else { continue };
        for rule in css.split("@font-face").skip(1) {
            let prop = |name: &str| Some(rule.split_once(&format!("{name}:"))?.1.split([';', '}']).next()?.trim());
            let (Some(family), Some(src)) = (prop("font-family"), prop("src")) else { continue };
            let Some(file) = src.split_once("/fonts/file/").and_then(|(_, f)| f.split(')').next()) else { continue };
            let path = dir.join("files").join(file);
            if seen.contains(&path) {
                continue;
            }
            // A file the canvas hasn't downloaded yet stays eligible for the next scan.
            let Ok(bytes) = std::fs::read(&path) else { continue };
            seen.insert(path);
            let Ok(ttf) = wuff::decompress_woff2(&bytes) else { continue };
            if !ttf_parser::Face::parse(&ttf, 0).is_ok_and(|f| ('a'..='z').all(|c| f.glyph_index(c).is_some())) {
                continue;
            }
            let db = Arc::make_mut(db);
            for id in db.load_font_source(fontdb::Source::Binary(Arc::new(ttf))) {
                let Some(mut face) = db.face(id).cloned() else { continue };
                db.remove_face(id);
                face.families =
                    vec![(family.trim_matches(['\'', '"']).to_string(), fontdb::Language::English_UnitedStates)];
                if prop("font-style") == Some("italic") {
                    face.style = fontdb::Style::Italic;
                }
                db.push_face_info(face);
            }
        }
    }
}

/// Just the bundled Geist fonts: instant to load, and what most designs use.
fn bundled_fonts() -> &'static fontdb::Database {
    static DB: OnceLock<fontdb::Database> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = fontdb::Database::new();
        for f in BUNDLED {
            db.load_font_data(f.to_vec());
        }
        db.set_sans_serif_family("Geist");
        db.set_monospace_family("Geist Mono");
        db
    })
}

/// Loads the font list in the background so the first export doesn't wait for it. `config_dir`
/// holds the downloaded Google Fonts.
pub fn preload_fonts(config_dir: &Path) {
    let _ = CONFIG_DIR.set(config_dir.to_path_buf());
    std::thread::spawn(fonts);
}

type FaceKey = (String, u16, bool);

/// Glyph advances per font face and character, cached because layout measures text a lot.
struct Advances {
    face: Option<fontdb::ID>,
    upm: f64,
    chars: std::collections::HashMap<char, f64>,
}

/// Width of one line of text with real glyph advances (falls back to 0.55 em per character).
pub fn measure(text: &str, s: &svg::TextStyle) -> f64 {
    thread_local! {
        static CACHE: std::cell::RefCell<(u64, std::collections::HashMap<FaceKey, Advances>)> = Default::default();
    }
    let lower = s.family.to_ascii_lowercase();
    let bundled = matches!(
        lower.as_str(),
        "geist" | "geist mono" | "sans-serif" | "system-ui" | "-apple-system" | "monospace" | "ui-monospace"
    );
    let with_db =
        |f: &mut dyn FnMut(&fontdb::Database) -> f64| -> f64 { if bundled { f(bundled_fonts()) } else { f(&fonts()) } };
    CACHE.with(|cache| {
        let (generation, cache) = &mut *cache.borrow_mut();
        // A downloaded font can fill a face that was missing before.
        let now = FONTS_GEN.load(Ordering::Relaxed);
        if *generation != now {
            (*generation, *cache) = (now, Default::default());
        }
        let entry = cache.entry((lower.clone(), s.weight, s.italic)).or_insert_with(|| {
            let mut found = (None, 1000.0);
            with_db(&mut |db| {
                let family = match lower.as_str() {
                    "sans-serif" | "system-ui" | "-apple-system" => fontdb::Family::SansSerif,
                    "monospace" | "ui-monospace" => fontdb::Family::Monospace,
                    "serif" => fontdb::Family::Serif,
                    _ => fontdb::Family::Name(s.family),
                };
                let q = fontdb::Query {
                    families: &[family, fontdb::Family::SansSerif],
                    weight: fontdb::Weight(s.weight),
                    style: if s.italic { fontdb::Style::Italic } else { fontdb::Style::Normal },
                    ..Default::default()
                };
                if let Some(id) = db.query(&q) {
                    let upm = db
                        .with_face_data(id, |d, i| ttf_parser::Face::parse(d, i).ok().map(|f| f.units_per_em() as f64))
                        .flatten();
                    found = (Some(id), upm.unwrap_or(1000.0));
                }
                0.0
            });
            Advances { face: found.0, upm: found.1, chars: Default::default() }
        });
        let Some(face) = entry.face else { return text.chars().count() as f64 * s.size * 0.55 };
        let missing: Vec<char> = text.chars().filter(|c| !entry.chars.contains_key(c)).collect();
        if !missing.is_empty() {
            let upm = entry.upm;
            let looked = {
                let mut out = Vec::new();
                with_db(&mut |db| {
                    db.with_face_data(face, |d, i| {
                        if let Ok(f) = ttf_parser::Face::parse(d, i) {
                            for c in &missing {
                                let adv = f
                                    .glyph_index(*c)
                                    .and_then(|g| f.glyph_hor_advance(g))
                                    .map(|a| a as f64)
                                    .unwrap_or(upm * 0.5);
                                out.push((*c, adv / upm));
                            }
                        }
                    });
                    0.0
                });
                out
            };
            entry.chars.extend(looked);
        }
        text.chars().map(|c| entry.chars.get(&c).copied().unwrap_or(0.5) * s.size + s.letter_spacing).sum()
    })
}

/// A standalone SVG for a node, with images embedded.
pub fn node_svg(doc: &Document, id: NodeId) -> Result<String, String> {
    let asset = |h: &str| doc.assets.get(h).map(|a| (a.mime.clone(), a.bytes.as_ref().clone()));
    svg::node_svg(doc, id, &svg::SvgOpts { measure: &measure, asset: &asset })
}

fn tree(svg: &str) -> Result<usvg::Tree, String> {
    let opts = usvg::Options { fontdb: fonts(), ..Default::default() };
    usvg::Tree::from_str(svg, &opts).map_err(|e| format!("couldn't parse the generated SVG: {e}"))
}

/// Rasterizes SVG at `scale` with resvg (used when no UI window is available to render).
pub fn rasterize(svg: &str, scale: f64) -> Result<image::RgbaImage, String> {
    let tree = tree(svg)?;
    let size = tree.size();
    let (w, h) =
        ((size.width() as f64 * scale).ceil().max(1.0) as u32, (size.height() as f64 * scale).ceil().max(1.0) as u32);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).ok_or("image too large")?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale as f32, scale as f32), &mut pixmap.as_mut());
    // tiny-skia stores premultiplied alpha.
    let mut img = image::RgbaImage::new(w, h);
    for (dst, src) in img.pixels_mut().zip(pixmap.pixels()) {
        let c = src.demultiply();
        *dst = image::Rgba([c.red(), c.green(), c.blue(), c.alpha()]);
    }
    Ok(img)
}

/// Encodes an image; JPG gets a white background since it has no alpha.
pub fn encode(img: &image::RgbaImage, format: Format, quality: u8) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut cur = std::io::Cursor::new(&mut out);
    match format {
        Format::Png => img.write_to(&mut cur, image::ImageFormat::Png),
        Format::Webp => image::DynamicImage::ImageRgba8(img.clone()).write_to(&mut cur, image::ImageFormat::WebP),
        Format::Jpg => {
            let mut rgb = image::RgbImage::new(img.width(), img.height());
            for (d, s) in rgb.pixels_mut().zip(img.pixels()) {
                let a = s[3] as u32;
                let mix = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
                *d = image::Rgb([mix(s[0]), mix(s[1]), mix(s[2])]);
            }
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cur, quality.clamp(1, 100)).encode_image(&rgb)
        }
        _ => return Err("not a raster format".into()),
    }
    .map_err(|e| e.to_string())?;
    Ok(out)
}

/// One PDF with a page per SVG, each page sized to its SVG.
pub fn pdf(svgs: &[String]) -> Result<Vec<u8>, String> {
    use krilla_svg::{SurfaceExt, SvgSettings};
    let mut doc = krilla::Document::new();
    for s in svgs {
        let t = tree(s)?;
        let (w, h) = (t.size().width(), t.size().height());
        let mut page = doc.start_page_with(krilla::page::PageSettings::from_wh(w, h).ok_or("page size out of range")?);
        let mut surface = page.surface();
        surface
            .draw_svg(&t, krilla::geom::Size::from_wh(w, h).unwrap(), SvgSettings::default())
            .ok_or("couldn't draw the page")?;
        surface.finish();
        page.finish();
    }
    doc.finish().map_err(|e| format!("couldn't write the PDF: {e:?}"))
}

/// Where exports go by default: `SPARK_DOWNLOADS_DIR`, else the user's Downloads folder.
pub fn downloads_dir() -> PathBuf {
    std::env::var_os("SPARK_DOWNLOADS_DIR")
        .map(PathBuf::from)
        .or_else(dirs::download_dir)
        .or_else(dirs::home_dir)
        .unwrap_or_else(std::env::temp_dir)
}

/// Writes `bytes` as `<name>.<ext>` in `dir`, adding " (1)", " (2)"… instead of overwriting.
pub fn save(dir: &Path, name: &str, ext: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
    let clean: String = name.chars().map(|c| if "/\\:*?\"<>|\n\r\t".contains(c) { '-' } else { c }).collect();
    let clean = clean.trim().trim_matches('.');
    let base = if clean.is_empty() { "Export" } else { clean };
    let path = (0..)
        .map(|i| dir.join(if i == 0 { format!("{base}.{ext}") } else { format!("{base} ({i}).{ext}") }))
        .find(|p| !p.exists())
        .unwrap();
    std::fs::write(&path, bytes).map_err(|e| format!("can't write {}: {e}", path.display()))?;
    Ok(path)
}

/// Exports one node in one format. Raster formats use a connected UI when `hub` has one.
pub async fn export_node(
    hub: Option<&Hub>,
    doc: &crate::hub::DocRef,
    id: NodeId,
    format: Format,
    scale: Scale,
    quality: u8,
) -> Result<(String, Vec<u8>), String> {
    if matches!(format, Format::Svg | Format::Pdf) {
        browser_layout(hub, doc, &[id]).await?;
    }
    let (file, name, svg, rect, code) = {
        let mut d = doc.lock().unwrap();
        crate::layout::ensure(&mut d.editor.doc, id);
        let n = d.editor.doc.node(id).ok_or_else(|| format!("node {id} not found"))?;
        let rect = d.editor.doc.measured.get(&id).copied();
        let svg = if matches!(format, Format::Svg | Format::Pdf)
            || (format.is_raster() && hub.is_none_or(|h| h.client_ids().is_empty()))
        {
            Some(node_svg(&d.editor.doc, id)?)
        } else {
            None
        };
        let code = match format {
            Format::Html | Format::Jsx | Format::Tailwind => {
                let f = match format {
                    Format::Html => codegen::Format::Html,
                    Format::Jsx => codegen::Format::Jsx,
                    _ => codegen::Format::Tailwind,
                };
                let assets = d.editor.doc.assets.clone();
                let url = move |h: &str| {
                    use base64::Engine;
                    assets
                        .get(h)
                        .map(|a| {
                            format!(
                                "data:{};base64,{}",
                                a.mime,
                                base64::engine::general_purpose::STANDARD.encode(a.bytes.as_slice())
                            )
                        })
                        .unwrap_or_default()
                };
                Some(codegen::code(
                    &d.editor.doc,
                    &[id],
                    &codegen::CodeOpts { format: f, include_ids: false, root_positioning: false, asset_url: &url },
                ))
            }
            _ => None,
        };
        (d.id().to_string(), n.name.clone(), svg, rect, code)
    };
    let bytes = match format {
        Format::Svg => svg.unwrap().into_bytes(),
        Format::Pdf => pdf(&[svg.unwrap()])?,
        Format::Html => format!("<!doctype html>\n<html>\n<head><meta charset=\"utf-8\"><title>{name}</title></head>\n<body style=\"margin:0\">\n{}</body>\n</html>\n", code.unwrap()).into_bytes(),
        Format::Jsx | Format::Tailwind => format!("export default function Design() {{\n  return (\n{}  );\n}}\n", code.unwrap().lines().map(|l| format!("    {l}\n")).collect::<String>()).into_bytes(),
        _ => {
            let r = rect.ok_or_else(|| format!("{name} has no measured size yet; open the file in Spark once"))?;
            let factor = scale.factor(r.w, r.h);
            let img = match (hub, svg) {
                (Some(h), None) => {
                    let png = h.render(&file, id, factor).await?;
                    image::load_from_memory(&png).map_err(|e| e.to_string())?.to_rgba8()
                }
                (_, Some(svg)) => rasterize(&svg, factor)?,
                (None, None) => unreachable!(),
            };
            encode(&img, format, quality)?
        }
    };
    Ok((name, bytes))
}

/// When a Spark window is connected, lays out the artboards holding `ids` in the browser and
/// records those boxes, so SVG and PDF match the canvas instead of Spark's own approximate
/// layout (which also stands in for artboards the canvas hasn't drawn up close).
pub async fn browser_layout(hub: Option<&Hub>, doc: &crate::hub::DocRef, ids: &[NodeId]) -> Result<(), String> {
    let Some(hub) = hub.filter(|h| !h.client_ids().is_empty()) else { return Ok(()) };
    let (file, boards) = {
        let d = doc.lock().unwrap();
        let mut boards: Vec<NodeId> = Vec::new();
        for b in ids.iter().filter_map(|id| d.editor.doc.artboard_of(*id)) {
            if !boards.contains(&b) {
                boards.push(b);
            }
        }
        (d.id().to_string(), boards)
    };
    for board in boards {
        let rects = hub.measure(&file, board).await?;
        let mut d = doc.lock().unwrap();
        // The artboard keeps its place; everything else is positioned from it.
        crate::layout::ensure(&mut d.editor.doc, board);
        let Some(origin) = d.editor.doc.measured.get(&board).copied() else { continue };
        for (id, r) in rects.as_object().into_iter().flatten() {
            let (Ok(id), Some(a)) = (id.parse::<NodeId>(), r.as_array()) else { continue };
            let n: Vec<f64> = a.iter().filter_map(serde_json::Value::as_f64).collect();
            if let [x, y, w, h] = n[..] {
                d.editor.doc.set_measured(id, spark_core::Rect { x: origin.x + x, y: origin.y + y, w, h });
            }
        }
    }
    Ok(())
}

/// Exports several nodes as one PDF, a page each, ordered top to bottom then left to right.
pub fn combined_pdf(doc: &mut Document, ids: &[NodeId]) -> Result<Vec<u8>, String> {
    for id in ids {
        crate::layout::ensure(doc, *id);
    }
    let mut ordered: Vec<(NodeId, spark_core::Rect)> = ids
        .iter()
        .map(|id| doc.measured.get(id).map(|r| (*id, *r)).ok_or_else(|| format!("{id} has no measured size yet")))
        .collect::<Result<_, _>>()?;
    ordered.sort_by(|a, b| a.1.y.total_cmp(&b.1.y).then(a.1.x.total_cmp(&b.1.x)));
    let svgs: Vec<String> = ordered.iter().map(|(id, _)| node_svg(doc, *id)).collect::<Result<_, _>>()?;
    pdf(&svgs)
}

/// An export setting: format, scale, filename suffix (empty = the scale's) and JPEG quality.
pub type Preset = (Format, Scale, String, u8);

const JPEG_QUALITY: u8 = 90;

/// Export presets from a JSON list like `[{"format": "png", "scale": "2x"}]` (empty = PNG 1x).
pub fn presets(v: &serde_json::Value) -> Result<Vec<Preset>, String> {
    let list = v.as_array().cloned().unwrap_or_default();
    if list.is_empty() {
        return Ok(vec![(Format::Png, Scale::Factor(1.0), String::new(), JPEG_QUALITY)]);
    }
    list.iter()
        .map(|p| {
            let format = Format::parse(p["format"].as_str().unwrap_or("png"))?;
            let scale = Scale::parse(p["scale"].as_str().unwrap_or("1x"))?;
            let quality = p["quality"].as_u64().map_or(JPEG_QUALITY, |q| q.clamp(1, 100) as u8);
            Ok((format, scale, p["suffix"].as_str().unwrap_or_default().to_string(), quality))
        })
        .collect()
}

/// Exports a node with each preset and saves the files. Returns the paths written.
pub async fn export_presets(
    hub: Option<&Hub>,
    doc: &crate::hub::DocRef,
    id: NodeId,
    presets: &[Preset],
    dir: &Path,
) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for (format, scale, suffix, quality) in presets {
        let (name, bytes) = export_node(hub, doc, id, *format, *scale, *quality).await?;
        let suffix = if suffix.is_empty() && format.is_raster() { scale.suffix() } else { suffix.clone() };
        paths.push(save(dir, &format!("{name}{suffix}"), format.ext(), &bytes)?);
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_and_names() {
        assert_eq!(Scale::parse("2x").unwrap(), Scale::Factor(2.0));
        assert_eq!(Scale::parse("512w").unwrap(), Scale::Width(512.0));
        assert_eq!(Scale::parse("1").unwrap(), Scale::Factor(1.0));
        assert!(Scale::parse("big").is_err());
        assert_eq!(Scale::Width(512.0).factor(256.0, 10.0), 2.0);
        assert_eq!(Scale::Factor(10.0).factor(1.0, 1.0), 4.0, "capped");
        assert_eq!(Scale::Factor(2.0).suffix(), "@2x");
        let dir = tempfile::tempdir().unwrap();
        let a = save(dir.path(), "Hero / Desktop", "png", b"1").unwrap();
        let b = save(dir.path(), "Hero / Desktop", "png", b"2").unwrap();
        assert!(a.ends_with("Hero - Desktop.png") && b.ends_with("Hero - Desktop (1).png"));
    }

    #[test]
    fn measures_with_real_fonts() {
        let s = svg::TextStyle { family: "Geist", size: 16.0, weight: 400, italic: false, letter_spacing: 0.0 };
        let w = measure("Hello world", &s);
        assert!(w > 60.0 && w < 110.0, "{w}");
        assert!(measure("Hello world", &svg::TextStyle { size: 32.0, ..s }) > w * 1.9);
    }

    #[test]
    fn loads_the_latin_subset_of_downloaded_fonts() {
        let dir = tempfile::tempdir().unwrap();
        let files =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../web/node_modules/@fontsource-variable/geist-mono/files");
        std::fs::create_dir(dir.path().join("files")).unwrap();
        let name = |subset: &str| format!("geist-mono-{subset}-wght-normal.woff2");
        let css = ["latin-ext", "latin"]
            .map(|s| format!("@font-face {{ font-family: 'Test Mono'; src: url(/fonts/file/{}); }}\n", name(s)))
            .concat();
        std::fs::write(dir.path().join("test-mono.css"), css).unwrap();
        let (mut seen, mut db) = (HashSet::new(), Arc::new(fontdb::Database::new()));
        // The canvas downloads each file on first use, so they arrive one at a time.
        let mut scan_after_download = |subset: &str| {
            std::fs::copy(format!("{files}/{}", name(subset)), dir.path().join("files").join(name(subset))).unwrap();
            load_downloaded(dir.path(), &mut seen, &mut db);
            db.len()
        };
        assert_eq!(scan_after_download("latin-ext"), 0, "not the Latin subset");
        assert_eq!(scan_after_download("latin"), 1, "the Latin file, downloaded after the first scan");
        assert_eq!(scan_after_download("latin"), 1, "loaded once");
        let q = fontdb::Query { families: &[fontdb::Family::Name("Test Mono")], ..Default::default() };
        assert!(db.query(&q).is_some(), "found by its CSS family name");
    }

    #[test]
    fn rasterizes_encodes_and_makes_pdf() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="#ff0000"/><text x="2" y="15" font-family="Geist" font-size="12">Hi</text></svg>"##;
        let img = rasterize(svg, 2.0).unwrap();
        assert_eq!((img.width(), img.height()), (80, 40));
        assert_eq!(img.get_pixel(1, 1).0, [255, 0, 0, 255]);
        for f in [Format::Png, Format::Jpg, Format::Webp] {
            let bytes = encode(&img, f, 90).unwrap();
            let back = image::load_from_memory(&bytes).unwrap();
            assert_eq!((back.width(), back.height()), (80, 40));
        }
        let p = pdf(&[svg.to_string(), svg.to_string()]).unwrap();
        assert!(p.starts_with(b"%PDF-"));
    }
}
