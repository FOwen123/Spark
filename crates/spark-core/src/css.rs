//! CSS in and out: parse declarations into a [`Style`] and compile a node's style to CSS.

use std::collections::BTreeMap;

use crate::{
    color::Color,
    node::{Node, NodeKind},
    style::{Border, Fill, Fit, Len, Shadow, Size, Stop, Style, fmt_num},
};

/// `backgroundColor` → `background-color`; kebab-case input is returned unchanged.
pub fn kebab(prop: &str) -> String {
    if prop.starts_with("--") {
        return prop.to_string();
    }
    let mut out = String::with_capacity(prop.len() + 4);
    for (i, ch) in prop.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            // Vendor prefixes: `WebkitLineClamp` → `-webkit-line-clamp`.
            if i > 0 || matches!(prop, p if p.starts_with("Webkit") || p.starts_with("Moz") || p.starts_with("Ms")) {
                out.push('-');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// `background-color` → `backgroundColor` (custom properties stay as they are).
pub fn camel(prop: &str) -> String {
    if prop.starts_with("--") {
        return prop.to_string();
    }
    let mut out = String::with_capacity(prop.len());
    let mut up = false;
    for (i, ch) in prop.chars().enumerate() {
        if ch == '-' {
            up = i > 0;
        } else if up {
            out.push(ch.to_ascii_uppercase());
            up = false;
        } else {
            out.push(ch);
        }
    }
    match prop.strip_prefix('-') {
        Some(rest) if !rest.starts_with("ms-") => {
            let mut c = out.chars();
            c.next().map(|f| f.to_ascii_uppercase().to_string() + c.as_str()).unwrap_or_default()
        }
        _ => out,
    }
}

/// Splits on `sep` outside parentheses and quotes.
pub fn split_top(s: &str, sep: char) -> Vec<&str> {
    split_top_by(s, |c| c == sep)
}

fn split_top_by(s: &str, is_sep: impl Fn(char) -> bool) -> Vec<&str> {
    let (mut depth, mut quote, mut start) = (0i32, None, 0);
    let mut parts = Vec::new();
    for (i, ch) in s.char_indices() {
        match ch {
            '"' | '\'' if quote == Some(ch) => quote = None,
            '"' | '\'' if quote.is_none() => quote = Some(ch),
            '(' if quote.is_none() => depth += 1,
            ')' if quote.is_none() => depth -= 1,
            c if is_sep(c) && depth == 0 && quote.is_none() => {
                parts.push(&s[start..i]);
                start = i + ch.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts.into_iter().map(str::trim).filter(|p| !p.is_empty()).collect()
}

fn words(s: &str) -> Vec<&str> {
    split_top_by(s, char::is_whitespace)
}

/// Parses an inline `style` attribute into (property, value) pairs.
pub fn parse_declarations(css: &str) -> Vec<(String, String)> {
    split_top(css, ';')
        .into_iter()
        .filter_map(|d| {
            let (p, v) = d.split_once(':')?;
            let (p, v) = (p.trim(), v.trim());
            (!p.is_empty() && !v.is_empty()).then(|| (kebab(p), v.to_string()))
        })
        .collect()
}

fn valid_prop(p: &str) -> bool {
    let body = p.strip_prefix("--").or_else(|| p.strip_prefix('-')).unwrap_or(p);
    !body.is_empty()
        && body.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || p.starts_with("--"))
        && body.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn keyword(v: &str) -> Option<String> {
    (!v.is_empty() && v.chars().all(|c| c.is_ascii_alphabetic() || c == '-' || c == ' ')).then(|| v.to_string())
}

fn len(v: &str) -> Option<Len> {
    Len::parse(v, false)
}

fn sides(v: &str) -> Option<[Len; 4]> {
    let w: Vec<Len> = words(v).into_iter().map(len).collect::<Option<_>>()?;
    Some(match w.len() {
        1 => [w[0].clone(), w[0].clone(), w[0].clone(), w[0].clone()],
        2 => [w[0].clone(), w[1].clone(), w[0].clone(), w[1].clone()],
        3 => [w[0].clone(), w[1].clone(), w[2].clone(), w[1].clone()],
        4 => [w[0].clone(), w[1].clone(), w[2].clone(), w[3].clone()],
        _ => return None,
    })
}

fn px_of(v: &str) -> Option<f64> {
    match v.trim() {
        "0" => Some(0.0),
        "thin" => Some(1.0),
        "medium" => Some(3.0),
        "thick" => Some(5.0),
        t => len(t)?.px(),
    }
}

fn angle(v: &str) -> Option<f64> {
    let v = v.trim();
    if let Some(d) = v.strip_suffix("deg") {
        return d.parse().ok();
    }
    if let Some(t) = v.strip_suffix("turn") {
        return t.parse::<f64>().ok().map(|t| t * 360.0);
    }
    if let Some(r) = v.strip_suffix("rad") {
        return r.parse::<f64>().ok().map(f64::to_degrees);
    }
    let dir = v.strip_prefix("to ")?;
    Some(match dir {
        "top" => 0.0,
        "right" => 90.0,
        "bottom" => 180.0,
        "left" => 270.0,
        "top right" | "right top" => 45.0,
        "bottom right" | "right bottom" => 135.0,
        "bottom left" | "left bottom" => 225.0,
        "top left" | "left top" => 315.0,
        _ => return None,
    })
}

fn stops(args: &[&str]) -> Option<Vec<Stop>> {
    let mut out: Vec<(Color, Option<f64>)> = Vec::new();
    for a in args {
        let w = words(a);
        let color: Color = w.first()?.parse().ok()?;
        let pos = match w.get(1) {
            Some(p) => Some(match len(p)? {
                Len::Pct(v) => v / 100.0,
                _ => return None,
            }),
            None => None,
        };
        out.push((color, pos));
    }
    if out.len() < 2 {
        return None;
    }
    let n = out.len() - 1;
    Some(
        out.into_iter()
            .enumerate()
            .map(|(i, (color, pos))| Stop { color, pos: pos.unwrap_or(i as f64 / n as f64) })
            .collect(),
    )
}

/// Splits an optional ` in <space>` suffix off a gradient's first argument.
fn split_space(arg: &str) -> (&str, Option<String>) {
    match arg.split_once(" in ") {
        Some((head, space)) if matches!(space.trim(), "srgb" | "oklab" | "oklch") => {
            (head.trim(), Some(space.trim().to_string()))
        }
        _ => (arg.trim(), None),
    }
}

/// Parses one background layer (gradient or `/assets/<hash>` image) into a fill.
pub fn parse_fill_layer(layer: &str) -> Option<Fill> {
    let l = layer.trim();
    if let Some(inner) = l.strip_prefix("linear-gradient(").and_then(|r| r.strip_suffix(')')) {
        let args = split_top(inner, ',');
        let (head, space) = split_space(args.first()?);
        let (a, rest) = match angle(head) {
            Some(a) => (a, &args[1..]),
            None if space.is_some() && head.is_empty() => (180.0, &args[1..]),
            None => (180.0, &args[..]),
        };
        return Some(Fill::Linear { angle: a, stops: stops(rest)?, space, hidden: false });
    }
    if let Some(inner) = l.strip_prefix("radial-gradient(").and_then(|r| r.strip_suffix(')')) {
        let args = split_top(inner, ',');
        let (head, space) = split_space(args.first()?);
        let rest = if head.parse::<Color>().is_ok() { &args[..] } else { &args[1..] };
        return Some(Fill::Radial { stops: stops(rest)?, space, hidden: false });
    }
    if let Some(inner) = l.strip_prefix("conic-gradient(").and_then(|r| r.strip_suffix(')')) {
        let args = split_top(inner, ',');
        let (head, space) = split_space(args.first()?);
        if head.parse::<Color>().is_ok() {
            return Some(Fill::Conic { angle: 0.0, stops: stops(&args)?, space, hidden: false });
        }
        let a = match head.strip_prefix("from ") {
            Some(f) => angle(f.split(" at ").next()?)?,
            None if head.starts_with("at ") || head.is_empty() => 0.0,
            None => return None,
        };
        return Some(Fill::Conic { angle: a, stops: stops(&args[1..])?, space, hidden: false });
    }
    let url = l.strip_prefix("url(")?.strip_suffix(')')?.trim().trim_matches(['"', '\'']);
    let hash = url.strip_prefix("/assets/")?;
    (!hash.is_empty() && hash.chars().all(|c| c.is_ascii_alphanumeric())).then(|| Fill::Image {
        asset: hash.to_string(),
        fit: Fit::Cover,
        hidden: false,
    })
}

fn parse_shadow(s: &str) -> Option<Shadow> {
    let mut inset = false;
    let mut color = None;
    let mut nums = Vec::new();
    for w in words(s) {
        if w == "inset" {
            inset = true;
        } else if let Some(n) = px_of(w) {
            nums.push(n);
        } else {
            color = Some(w.parse::<Color>().ok()?);
        }
    }
    if !(2..=4).contains(&nums.len()) {
        return None;
    }
    Some(Shadow {
        inset,
        x: nums[0],
        y: nums[1],
        blur: *nums.get(2).unwrap_or(&0.0),
        spread: *nums.get(3).unwrap_or(&0.0),
        color: color.unwrap_or(Color::Oklch { l: 0.0, c: 0.0, h: 0.0, a: 1.0 }),
        hidden: false,
    })
}

fn parse_border(v: &str) -> Option<(f64, String, Color)> {
    let (mut width, mut style, mut color) = (3.0, None, Color::BLACK);
    for w in words(v) {
        if let Some(n) = px_of(w) {
            width = n;
        } else if matches!(w, "solid" | "dashed" | "dotted" | "double" | "none" | "hidden" | "groove" | "ridge") {
            style = Some(w.to_string());
        } else {
            color = w.parse().ok()?;
        }
    }
    let style = style?;
    if style == "none" || style == "hidden" {
        width = 0.0;
    }
    Some((width, style, color))
}

fn single_filter(v: &str, name: &str) -> Option<f64> {
    let inner = v.trim().strip_prefix(name)?.strip_prefix('(')?.strip_suffix(')')?;
    px_of(inner)
}

/// Filter functions Spark models besides blur.
const FILTERS: [&str; 7] = ["brightness", "contrast", "grayscale", "hue-rotate", "invert", "saturate", "sepia"];

/// A `filter` list made only of blur and [`FILTERS`] functions, as (blur, others).
fn parse_filters(v: &str) -> Option<(Option<f64>, BTreeMap<String, f64>)> {
    let (mut blur, mut out) = (None, BTreeMap::new());
    for f in words(v) {
        let (name, rest) = f.split_once('(')?;
        let arg = rest.strip_suffix(')')?.trim();
        if name == "blur" {
            blur = Some(px_of(arg)?);
        } else if FILTERS.contains(&name) {
            let n = if name == "hue-rotate" {
                angle(arg)?
            } else {
                match len(arg)? {
                    Len::Pct(p) => p / 100.0,
                    Len::Px(n) | Len::Num(n) => n,
                    _ => return None,
                }
            };
            out.insert(name.to_string(), n);
        } else {
            return None;
        }
    }
    Some((blur, out))
}

fn side_index(s: &str) -> Option<usize> {
    ["top", "right", "bottom", "left"].iter().position(|x| *x == s)
}

fn border_mut(style: &mut Style) -> &mut Border {
    style.border.get_or_insert_with(|| Border { width: [0.0; 4], style: "solid".into(), color: Color::BLACK })
}

/// Applies one CSS declaration. Unmodelled properties go to `raw`. Returns an error message for
/// declarations that are ignored (bad property names or values Spark can't use).
pub fn apply_decl(style: &mut Style, prop: &str, value: &str) -> Result<(), String> {
    let prop = kebab(prop.trim());
    let value = value.trim().trim_end_matches("!important").trim();
    if !valid_prop(&prop) || value.is_empty() {
        return Err(format!("`{prop}: {value}` is not a valid declaration"));
    }
    let bad = || format!("unsupported value `{value}` for `{prop}`");
    let mut to_raw = false;
    match prop.as_str() {
        _ if matches!(value, "inherit" | "initial" | "unset" | "revert") => to_raw = true,
        "width" | "height" => {
            let size = if value == "auto" { None } else { Some(value.parse::<Size>().map_err(|_| bad())?) };
            if prop == "width" { style.width = size } else { style.height = size }
        }
        "min-width" | "max-width" | "min-height" | "max-height" => {
            let l = if matches!(value, "none" | "auto") {
                None
            } else {
                match len(value) {
                    Some(l) => Some(l),
                    None => {
                        to_raw = true;
                        None
                    }
                }
            };
            if !to_raw {
                *match prop.as_str() {
                    "min-width" => &mut style.min_width,
                    "max-width" => &mut style.max_width,
                    "min-height" => &mut style.min_height,
                    _ => &mut style.max_height,
                } = l;
            }
        }
        "position" => {
            style.position = Some(
                keyword(value)
                    .filter(|k| matches!(k.as_str(), "static" | "relative" | "absolute" | "fixed" | "sticky"))
                    .ok_or_else(bad)?,
            )
        }
        "left" | "top" | "right" | "bottom" => {
            let l = if value == "auto" { None } else { Some(len(value).ok_or_else(bad)?) };
            *match prop.as_str() {
                "left" => &mut style.left,
                "top" => &mut style.top,
                "right" => &mut style.right,
                _ => &mut style.bottom,
            } = l;
        }
        "rotate" => style.rotate = Some(angle(value).ok_or_else(bad)?),
        "transform" => match value.strip_prefix("rotate(").and_then(|r| r.strip_suffix(')')).and_then(angle) {
            Some(a) => style.rotate = Some(a),
            None if value == "none" => style.rotate = None,
            None => to_raw = true,
        },
        "z-index" => style.z_index = if value == "auto" { None } else { Some(value.parse().map_err(|_| bad())?) },
        "display" => style.display = Some(keyword(value).ok_or_else(bad)?),
        "flex-direction" => style.flex_direction = Some(keyword(value).ok_or_else(bad)?),
        "flex-wrap" => style.flex_wrap = Some(keyword(value).ok_or_else(bad)?),
        "align-items" => style.align_items = Some(keyword(value).ok_or_else(bad)?),
        "justify-content" => style.justify_content = Some(keyword(value).ok_or_else(bad)?),
        "align-self" => style.align_self = Some(keyword(value).ok_or_else(bad)?),
        "gap" => match len(value) {
            Some(l) => style.gap = Some(l),
            None => to_raw = true,
        },
        "padding" => style.padding = Some(sides(value).ok_or_else(bad)?),
        "padding-top" | "padding-right" | "padding-bottom" | "padding-left" => {
            let i = side_index(&prop[8..]).unwrap();
            let p = style.padding.get_or_insert_with(|| std::array::from_fn(|_| Len::Px(0.0)));
            p[i] = len(value).ok_or_else(bad)?;
        }
        "flex" => {
            let (g, s, b) = match value {
                "none" => (0.0, 0.0, None),
                "auto" => (1.0, 1.0, None),
                _ => {
                    let w = words(value);
                    let g = w.first().and_then(|v| v.parse::<f64>().ok()).ok_or_else(bad)?;
                    let s = w.get(1).and_then(|v| v.parse::<f64>().ok()).unwrap_or(1.0);
                    let b = match w.last().filter(|_| w.len() > 1 && w.last().unwrap().parse::<f64>().is_err()) {
                        Some(b) if *b == "auto" => None,
                        Some(b) => Some(len(b).ok_or_else(bad)?),
                        None => Some(Len::Pct(0.0)),
                    };
                    (g, s, b)
                }
            };
            style.flex_grow = Some(g);
            style.flex_shrink = Some(s);
            style.flex_basis = b;
        }
        "flex-grow" => style.flex_grow = Some(value.parse().map_err(|_| bad())?),
        "flex-shrink" => style.flex_shrink = Some(value.parse().map_err(|_| bad())?),
        "flex-basis" => style.flex_basis = if value == "auto" { None } else { Some(len(value).ok_or_else(bad)?) },
        "overflow" => style.overflow = Some(keyword(value).ok_or_else(bad)?),
        "opacity" => {
            let o = match len(value) {
                Some(Len::Pct(p)) => p / 100.0,
                _ => value.parse::<f64>().map_err(|_| bad())?,
            };
            style.opacity = if o >= 1.0 { None } else { Some(o.max(0.0)) };
        }
        "mix-blend-mode" => style.blend = if value == "normal" { None } else { Some(keyword(value).ok_or_else(bad)?) },
        "border-radius" => {
            if value.contains('/') {
                to_raw = true;
            } else {
                let s = sides(value).ok_or_else(bad)?;
                style.radius = if s.iter().all(|l| *l == Len::Px(0.0)) { None } else { Some(s) };
            }
        }
        "border-top-left-radius"
        | "border-top-right-radius"
        | "border-bottom-right-radius"
        | "border-bottom-left-radius" => {
            let i = ["top-left", "top-right", "bottom-right", "bottom-left"]
                .iter()
                .position(|c| prop[7..].starts_with(c))
                .unwrap();
            let r = style.radius.get_or_insert_with(|| std::array::from_fn(|_| Len::Px(0.0)));
            r[i] = len(value).ok_or_else(bad)?;
        }
        "background-color" => {
            let c: Color = value.parse().map_err(|_| bad())?;
            let solid = Fill::Solid { color: c, hidden: false };
            match style.fills.last_mut() {
                Some(last @ Fill::Solid { .. }) => *last = solid,
                _ => style.fills.push(solid),
            }
        }
        "background" | "background-image" => {
            if value == "none" {
                if prop == "background" {
                    style.fills.clear();
                } else {
                    style.fills.retain(|f| matches!(f, Fill::Solid { .. }));
                }
                return Ok(());
            }
            let layers = split_top(value, ',');
            let mut fills = Vec::new();
            for (i, layer) in layers.iter().enumerate() {
                let last = i + 1 == layers.len();
                if prop == "background"
                    && last
                    && let Ok(c) = layer.parse::<Color>()
                {
                    fills.push(Fill::Solid { color: c, hidden: false });
                    continue;
                }
                let mut parsed = None;
                for w in if prop == "background" { words(layer) } else { vec![*layer] } {
                    if let Some(f) = parse_fill_layer(w) {
                        parsed = Some(f);
                    } else if prop == "background"
                        && last
                        && let Ok(c) = w.parse::<Color>()
                    {
                        fills.push(Fill::Solid { color: c, hidden: false });
                    }
                }
                match parsed {
                    Some(f) => fills
                        .insert(fills.len() - usize::from(matches!(fills.last(), Some(Fill::Solid { .. }) if last)), f),
                    None => {
                        to_raw = true;
                        break;
                    }
                }
            }
            if !to_raw {
                if prop == "background-image"
                    && let Some(solid @ Fill::Solid { .. }) = style.fills.last().cloned()
                {
                    fills.push(solid);
                }
                style.fills = fills;
            }
        }
        "background-size" | "background-repeat" if style.fills.iter().any(|f| matches!(f, Fill::Image { .. })) => {
            let fit = match value {
                "cover" => Some(Fit::Cover),
                "contain" => Some(Fit::Contain),
                "100% 100%" => Some(Fit::Fill),
                "repeat" => Some(Fit::Tile),
                "no-repeat" => None,
                _ => {
                    to_raw = true;
                    None
                }
            };
            if let Some(new_fit) = fit {
                for f in &mut style.fills {
                    if let Fill::Image { fit, .. } = f {
                        *fit = new_fit;
                    }
                }
            }
        }
        "background-position" if value == "center" || value == "50% 50%" => {}
        "border" | "border-top" | "border-right" | "border-bottom" | "border-left" => {
            if matches!(value, "none" | "0") {
                match side_index(prop.trim_start_matches("border").trim_start_matches('-')) {
                    Some(i) => {
                        if let Some(b) = &mut style.border {
                            b.width[i] = 0.0;
                        }
                    }
                    None => style.border = None,
                }
            } else {
                let (w, s, c) = parse_border(value).ok_or_else(bad)?;
                let side = side_index(prop.trim_start_matches("border").trim_start_matches('-'));
                let b = border_mut(style);
                match side {
                    Some(i) => b.width[i] = w,
                    None => b.width = [w; 4],
                }
                b.style = s;
                b.color = c;
            }
        }
        "border-width" => {
            let s = sides(value).ok_or_else(bad)?;
            let px: Vec<f64> = s.iter().map(|l| l.px().ok_or_else(bad)).collect::<Result<_, _>>()?;
            border_mut(style).width = [px[0], px[1], px[2], px[3]];
        }
        "border-top-width" | "border-right-width" | "border-bottom-width" | "border-left-width" => {
            let i = side_index(prop[7..].trim_end_matches("-width")).unwrap();
            border_mut(style).width[i] = px_of(value).ok_or_else(bad)?;
        }
        "border-style" => border_mut(style).style = keyword(value).ok_or_else(bad)?,
        "border-color" => border_mut(style).color = value.parse().map_err(|_| bad())?,
        "box-shadow" => {
            style.shadows = if value == "none" {
                Vec::new()
            } else {
                split_top(value, ',').into_iter().map(parse_shadow).collect::<Option<_>>().ok_or_else(bad)?
            }
        }
        "filter" => match parse_filters(value) {
            Some((blur, filters)) => (style.blur, style.filters) = (blur, filters),
            None if value == "none" => (style.blur, style.filters) = (None, BTreeMap::new()),
            None => to_raw = true,
        },
        "backdrop-filter" | "-webkit-backdrop-filter" => match single_filter(value, "blur") {
            Some(b) => style.backdrop_blur = Some(b),
            None if value == "none" => style.backdrop_blur = None,
            None => to_raw = true,
        },
        "font-family" => style.font_family = Some(value.to_string()),
        "font-size" => match len(value) {
            Some(l) => style.font_size = Some(l),
            None => to_raw = true,
        },
        "font-weight" => {
            style.font_weight = Some(match value {
                "normal" => 400,
                "bold" => 700,
                v => match v.parse::<u16>() {
                    Ok(w) if (1..=1000).contains(&w) => w,
                    _ => return Err(bad()),
                },
            })
        }
        "font-style" => style.font_style = if value == "normal" { None } else { Some(keyword(value).ok_or_else(bad)?) },
        "line-height" => {
            style.line_height = if value == "normal" { None } else { Some(Len::parse(value, true).ok_or_else(bad)?) }
        }
        "letter-spacing" => {
            style.letter_spacing = if value == "normal" { None } else { Some(len(value).ok_or_else(bad)?) }
        }
        "text-align" => style.text_align = Some(keyword(value).ok_or_else(bad)?),
        "color" => style.color = Some(value.parse().map_err(|_| bad())?),
        "text-decoration" | "text-decoration-line" => {
            style.text_decoration = if value == "none" { None } else { Some(value.to_string()) }
        }
        "text-transform" => {
            style.text_transform = if value == "none" { None } else { Some(keyword(value).ok_or_else(bad)?) }
        }
        _ => to_raw = true,
    }
    if to_raw {
        style.raw.insert(prop, value.to_string());
    } else {
        style.raw.remove(&prop);
    }
    Ok(())
}

/// Removes a CSS property (typed or raw) from the style.
pub fn remove_decl(style: &mut Style, prop: &str) {
    let prop = kebab(prop);
    style.raw.remove(&prop);
    let field = match prop.as_str() {
        "background" | "background-color" | "background-image" => "fills",
        "border-radius" => "radius",
        "mix-blend-mode" => "blend",
        "box-shadow" => "shadows",
        "filter" => {
            style.filters.clear();
            "blur"
        }
        "backdrop-filter" => "backdropBlur",
        "transform" => "rotate",
        p if p.starts_with("border") => "border",
        p if p.starts_with("padding") => "padding",
        "flex" => {
            style.flex_grow = None;
            style.flex_shrink = None;
            style.flex_basis = None;
            return;
        }
        p => return drop(style.set_field(&camel(p), serde_json::Value::Null)),
    };
    let _ = style.set_field(field, serde_json::Value::Null);
}

/// Layout facts about a node's parent that change how its own style compiles.
#[derive(Clone, Copy, Default)]
pub struct ParentCtx {
    pub flex: bool,
    pub row: bool,
}

impl ParentCtx {
    pub fn of(style: &Style) -> ParentCtx {
        ParentCtx { flex: style.is_flex(), row: style.is_row() }
    }
}

fn corners(r: &[Len; 4]) -> String {
    if r.iter().all(|l| *l == r[0]) {
        r[0].to_string()
    } else {
        r.iter().map(Len::to_string).collect::<Vec<_>>().join(" ")
    }
}

/// The first argument's trailing ` in <space>`, or nothing.
fn space_css(space: &Option<String>) -> String {
    space.as_ref().map(|s| format!(" in {s}")).unwrap_or_default()
}

fn stops_css(stops: &[Stop]) -> String {
    stops.iter().map(|s| format!("{} {}%", s.color.to_css(), fmt_num(s.pos * 100.0))).collect::<Vec<_>>().join(", ")
}

fn shadow_css(s: &Shadow) -> String {
    format!(
        "{}{}px {}px {}px {}px {}",
        if s.inset { "inset " } else { "" },
        fmt_num(s.x),
        fmt_num(s.y),
        fmt_num(s.blur),
        fmt_num(s.spread),
        s.color.to_css()
    )
}

/// Compiles a node's style to CSS declarations (kebab-case), in a stable order.
pub fn compile(node: &Node, parent: ParentCtx) -> Vec<(String, String)> {
    let s = &node.style;
    let mut out: Vec<(String, String)> = Vec::with_capacity(16);
    let mut put = |k: &str, v: String| out.push((k.to_string(), v));
    let text = node.kind == NodeKind::Text;
    let absolute = matches!(s.position.as_deref(), Some("absolute" | "fixed"));

    for (axis, size) in [("width", &s.width), ("height", &s.height)] {
        match size {
            Some(Size::Fixed(l)) => put(axis, l.to_string()),
            // An absolute box with `auto` height already fits its content. macOS WebKit collapses
            // `fit-content` there when a child's height is a percentage.
            Some(Size::Hug) if absolute && axis == "height" && !text => put(axis, "auto".into()),
            Some(Size::Hug) => put(axis, if text { "max-content" } else { "fit-content" }.into()),
            Some(Size::Fill) => {
                let main = parent.flex && (parent.row == (axis == "width"));
                if main {
                    put("flex-grow", "1".into());
                    put("flex-basis", "0px".into());
                    put(if axis == "width" { "min-width" } else { "min-height" }, "0px".into());
                } else if parent.flex {
                    put("align-self", "stretch".into());
                } else {
                    put(axis, "100%".into());
                }
            }
            None => {}
        }
    }
    for (k, v) in [
        ("min-width", &s.min_width),
        ("max-width", &s.max_width),
        ("min-height", &s.min_height),
        ("max-height", &s.max_height),
    ] {
        if let Some(v) = v {
            put(k, v.to_string());
        }
    }
    if let Some(p) = &s.position {
        put("position", p.clone());
    }
    for (k, v) in [("left", &s.left), ("top", &s.top), ("right", &s.right), ("bottom", &s.bottom)] {
        if let Some(v) = v {
            put(k, v.to_string());
        }
    }
    if let Some(r) = s.rotate.filter(|r| *r != 0.0) {
        put("rotate", format!("{}deg", fmt_num(r)));
    }
    if let Some(z) = s.z_index {
        put("z-index", z.to_string());
    }
    for (k, v) in [
        ("display", &s.display),
        ("flex-direction", &s.flex_direction),
        ("flex-wrap", &s.flex_wrap),
        ("align-items", &s.align_items),
        ("justify-content", &s.justify_content),
        ("align-self", &s.align_self),
    ] {
        if let Some(v) = v {
            put(k, v.clone());
        }
    }
    if let Some(g) = &s.gap {
        put("gap", g.to_string());
    }
    if let Some(p) = &s.padding {
        let [t, r, b, l] = p;
        put(
            "padding",
            if t == r && r == b && b == l {
                t.to_string()
            } else if t == b && r == l {
                format!("{t} {r}")
            } else {
                format!("{t} {r} {b} {l}")
            },
        );
    }
    if let Some(g) = s.flex_grow {
        put("flex-grow", fmt_num(g));
    }
    if let Some(v) = s.flex_shrink {
        put("flex-shrink", fmt_num(v));
    }
    if let Some(b) = &s.flex_basis {
        put("flex-basis", b.to_string());
    }
    if let Some(o) = &s.overflow {
        put("overflow", o.clone());
    }
    if let Some(o) = s.opacity {
        put("opacity", fmt_num(o));
    }
    if let Some(b) = &s.blend {
        put("mix-blend-mode", b.clone());
    }
    if let Some(r) = &s.radius {
        put("border-radius", corners(r));
    }
    let fills: Vec<&Fill> = s.fills.iter().filter(|f| !f.hidden()).collect();
    {
        if let [Fill::Solid { color, .. }] = fills[..] {
            put("background-color", color.to_css());
        } else if !fills.is_empty() {
            let last_solid = matches!(fills.last(), Some(Fill::Solid { .. }));
            let mut images = Vec::new();
            let (mut sizes, mut repeats) = (Vec::new(), Vec::new());
            for (i, f) in fills.iter().enumerate() {
                let (img, size, repeat) = match f {
                    Fill::Solid { color, .. } if last_solid && i + 1 == fills.len() => {
                        put("background-color", color.to_css());
                        continue;
                    }
                    Fill::Solid { color, .. } => {
                        (format!("linear-gradient({0}, {0})", color.to_css()), "auto", "no-repeat")
                    }
                    Fill::Linear { angle, stops, space, .. } => (
                        format!("linear-gradient({}deg{}, {})", fmt_num(*angle), space_css(space), stops_css(stops)),
                        "auto",
                        "no-repeat",
                    ),
                    Fill::Radial { stops, space, .. } => (
                        format!("radial-gradient(circle{}, {})", space_css(space), stops_css(stops)),
                        "auto",
                        "no-repeat",
                    ),
                    Fill::Conic { angle, stops, space, .. } => (
                        format!(
                            "conic-gradient(from {}deg{}, {})",
                            fmt_num(*angle),
                            space_css(space),
                            stops_css(stops)
                        ),
                        "auto",
                        "no-repeat",
                    ),
                    Fill::Image { asset, fit, .. } => (
                        format!("url(\"/assets/{asset}\")"),
                        match fit {
                            Fit::Cover => "cover",
                            Fit::Contain => "contain",
                            Fit::Fill => "100% 100%",
                            Fit::Tile => "auto",
                        },
                        if *fit == Fit::Tile { "repeat" } else { "no-repeat" },
                    ),
                };
                images.push(img);
                sizes.push(size);
                repeats.push(repeat);
            }
            if !images.is_empty() {
                put("background-image", images.join(", "));
                if sizes.iter().any(|s| *s != "auto") {
                    put("background-size", sizes.join(", "));
                    put("background-position", "center".into());
                }
                if repeats.iter().any(|r| *r != "repeat") {
                    put("background-repeat", repeats.join(", "));
                }
            }
        }
    }
    if let Some(b) = &s.border {
        let [t, r, btm, l] = b.width;
        if t == r && r == btm && btm == l {
            if t > 0.0 {
                put("border", format!("{}px {} {}", fmt_num(t), b.style, b.color.to_css()));
            }
        } else {
            put("border-width", b.width.map(|w| format!("{}px", fmt_num(w))).join(" "));
            put("border-style", b.style.clone());
            put("border-color", b.color.to_css());
        }
    }
    let shadows: Vec<String> = s.shadows.iter().filter(|s| !s.hidden).map(shadow_css).collect();
    if !shadows.is_empty() {
        put("box-shadow", shadows.join(", "));
    }
    let mut filter: Vec<String> = s.blur.map(|b| format!("blur({}px)", fmt_num(b))).into_iter().collect();
    for (k, v) in &s.filters {
        filter.push(format!("{k}({}{})", fmt_num(*v), if k == "hue-rotate" { "deg" } else { "" }));
    }
    if !filter.is_empty() {
        put("filter", filter.join(" "));
    }
    if let Some(b) = s.backdrop_blur {
        put("backdrop-filter", format!("blur({}px)", fmt_num(b)));
    }
    if let Some(f) = &s.font_family {
        put("font-family", f.clone());
    }
    if let Some(f) = &s.font_size {
        put("font-size", f.to_string());
    }
    if let Some(w) = s.font_weight {
        put("font-weight", w.to_string());
    }
    if let Some(f) = &s.font_style {
        put("font-style", f.clone());
    }
    if let Some(l) = &s.line_height {
        put("line-height", l.to_string());
    }
    if let Some(l) = &s.letter_spacing {
        put("letter-spacing", l.to_string());
    }
    if let Some(t) = &s.text_align {
        put("text-align", t.clone());
    }
    if let Some(c) = &s.color {
        put("color", c.to_css());
    }
    if let Some(t) = &s.text_decoration {
        put("text-decoration", t.clone());
    }
    if let Some(t) = &s.text_transform {
        put("text-transform", t.clone());
    }
    if text && !s.raw.contains_key("white-space") {
        put("white-space", "pre-wrap".into());
    }
    for (k, v) in &s.raw {
        match out.iter_mut().find(|(ok, _)| ok == k) {
            Some(slot) => slot.1 = v.clone(),
            None => out.push((k.clone(), v.clone())),
        }
    }
    if !s.disabled.is_empty() {
        out.retain(|(k, _)| {
            !s.disabled.iter().any(|d| k == d || (k.starts_with(d.as_str()) && k[d.len()..].starts_with('-')))
        });
    }
    out
}

/// `prop: value; ...` text for a compiled declaration list.
pub fn to_inline(decls: &[(String, String)]) -> String {
    decls.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{id::NodeId, node::Node};

    fn styled(css: &str) -> Style {
        let mut s = Style::default();
        for (p, v) in parse_declarations(css) {
            apply_decl(&mut s, &p, &v).unwrap();
        }
        s
    }

    fn compiled(kind: NodeKind, css: &str, parent: ParentCtx) -> String {
        let mut n = Node::new(NodeId::new(1, 0), kind);
        n.style = styled(css);
        to_inline(&compile(&n, parent))
    }

    #[test]
    fn case_conversion() {
        assert_eq!(kebab("backgroundColor"), "background-color");
        assert_eq!(kebab("WebkitLineClamp"), "-webkit-line-clamp");
        assert_eq!(kebab("--color-brand"), "--color-brand");
        assert_eq!(camel("background-color"), "backgroundColor");
        assert_eq!(camel("-webkit-line-clamp"), "WebkitLineClamp");
    }

    #[test]
    fn parses_layout_and_box_properties() {
        let s = styled(
            "display:flex; flex-direction: column; gap: 12px; padding: 8px 16px; width: 320px; height: fit-content; align-items:center",
        );
        assert!(s.is_flex() && !s.is_row());
        assert_eq!(s.gap, Some(Len::Px(12.0)));
        assert_eq!(s.padding.as_ref().unwrap()[1], Len::Px(16.0));
        assert_eq!(s.width, Some(Size::Fixed(Len::Px(320.0))));
        assert_eq!(s.height, Some(Size::Hug));
        let s = styled("flex: 1; padding-left: 4px");
        assert_eq!((s.flex_grow, s.flex_shrink, s.flex_basis.clone()), (Some(1.0), Some(1.0), Some(Len::Pct(0.0))));
        assert_eq!(s.padding.unwrap()[3], Len::Px(4.0));
    }

    #[test]
    fn parses_design_properties() {
        let s = styled(
            "background: linear-gradient(to right, #fff, #000 80%), #ff0000; border-radius: 8px; border: 1px dashed #ccc; \
             box-shadow: 0 4px 16px rgb(0 0 0 / 0.12), inset 0 0 0 1px #000; opacity: 0.5; filter: blur(4px)",
        );
        assert_eq!(s.fills.len(), 2);
        assert!(matches!(&s.fills[0], Fill::Linear { angle, stops, .. } if *angle == 90.0 && stops[1].pos == 0.8));
        assert!(matches!(&s.fills[1], Fill::Solid { .. }));
        assert_eq!(s.border.as_ref().unwrap().style, "dashed");
        assert_eq!(s.shadows.len(), 2);
        assert!(s.shadows[1].inset);
        assert_eq!((s.opacity, s.blur), (Some(0.5), Some(4.0)));
        let s = styled("border-bottom: 2px solid black; font: 12px serif; font-weight: bold; letter-spacing: -0.02em");
        assert_eq!(s.border.unwrap().width, [0.0, 0.0, 2.0, 0.0]);
        assert_eq!(s.raw.get("font").map(String::as_str), Some("12px serif"));
        assert_eq!(s.font_weight, Some(700));
    }

    #[test]
    fn rejects_bad_declarations() {
        let mut s = Style::default();
        assert!(apply_decl(&mut s, "width", "banana").is_err());
        assert!(apply_decl(&mut s, "not a prop", "1").is_err());
        assert!(apply_decl(&mut s, "color", "notacolor").is_err());
        assert!(apply_decl(&mut s, "grid-template-columns", "1fr 1fr").is_ok());
        assert_eq!(s.raw.len(), 1);
    }

    #[test]
    fn compiles_to_css() {
        let css = compiled(
            NodeKind::Frame,
            "width: 100px; height: 50px; background-color: #ff0000; border-radius: 4px",
            ParentCtx::default(),
        );
        assert_eq!(css, "width: 100px; height: 50px; border-radius: 4px; background-color: #ff0000");
        let css = compiled(NodeKind::Text, "width: hug; color: #000; font-size: 14px", ParentCtx::default());
        assert_eq!(css, "width: max-content; font-size: 14px; color: #000000; white-space: pre-wrap");
    }

    #[test]
    fn absolute_hug_height_compiles_to_auto() {
        let css =
            compiled(NodeKind::Frame, "position: absolute; width: 410px; height: fit-content", ParentCtx::default());
        assert!(css.starts_with("width: 410px; height: auto;"), "{css}");
        let css = compiled(NodeKind::Frame, "width: 410px; height: fit-content", ParentCtx::default());
        assert!(css.starts_with("width: 410px; height: fit-content"), "{css}");
    }

    #[test]
    fn fill_sizing_depends_on_parent_layout() {
        let row = ParentCtx { flex: true, row: true };
        assert_eq!(compiled(NodeKind::Frame, "width: fill", row), "flex-grow: 1; flex-basis: 0px; min-width: 0px");
        assert_eq!(compiled(NodeKind::Frame, "height: fill", row), "align-self: stretch");
        assert_eq!(compiled(NodeKind::Frame, "width: fill", ParentCtx::default()), "width: 100%");
    }

    #[test]
    fn raw_overrides_and_disabled_properties() {
        let mut n = Node::new(NodeId::new(1, 0), NodeKind::Frame);
        n.style = styled("background-color: red; border: 1px solid #000; cursor: pointer");
        n.style.disabled.insert("border".into());
        assert_eq!(to_inline(&compile(&n, ParentCtx::default())), "background-color: #ff0000; cursor: pointer");
    }

    #[test]
    fn multiple_fills_compile_to_layers() {
        let mut n = Node::new(NodeId::new(1, 0), NodeKind::Frame);
        n.style = styled("background-image: url(/assets/abc123); background-color: #fff");
        let css = to_inline(&compile(&n, ParentCtx::default()));
        assert_eq!(
            css,
            "background-color: #ffffff; background-image: url(\"/assets/abc123\"); background-size: cover; \
             background-position: center; background-repeat: no-repeat"
        );
    }

    #[test]
    fn conic_gradients_color_spaces_and_filters_round_trip() {
        let css = "background: conic-gradient(from 90deg in oklch, #fff, #000); filter: blur(2px) brightness(1.2) hue-rotate(30deg)";
        let s = styled(css);
        assert!(
            matches!(&s.fills[0], Fill::Conic { angle, space, .. } if *angle == 90.0 && space.as_deref() == Some("oklch"))
        );
        assert_eq!(
            (s.blur, s.filters.get("brightness"), s.filters.get("hue-rotate")),
            (Some(2.0), Some(&1.2), Some(&30.0))
        );
        let out = compiled(NodeKind::Frame, css, ParentCtx::default());
        assert!(out.contains("conic-gradient(from 90deg in oklch, #ffffff 0%, #000000 100%)"), "{out}");
        assert!(out.contains("filter: blur(2px) brightness(1.2) hue-rotate(30deg)"), "{out}");
        let s = styled("background: linear-gradient(90deg in oklab, #fff, #000)");
        assert!(
            matches!(&s.fills[0], Fill::Linear { angle, space, .. } if *angle == 90.0 && space.as_deref() == Some("oklab"))
        );
        let mut s = styled(css);
        remove_decl(&mut s, "filter");
        assert!(s.blur.is_none() && s.filters.is_empty());
    }
}
