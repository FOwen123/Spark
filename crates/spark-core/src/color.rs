//! Colors are stored as OKLCH (or a token reference) and accept any CSS color on input.

use std::{fmt, str::FromStr};

use crate::id::string_serde;

#[derive(Clone, PartialEq, Debug)]
pub enum Color {
    /// Lightness 0..1, chroma, hue in degrees, alpha 0..1.
    Oklch { l: f64, c: f64, h: f64, a: f64 },
    /// A token reference such as `--color-brand`.
    Var(String),
}

string_serde!(Color);

#[derive(Debug, thiserror::Error)]
#[error("invalid color `{0}`")]
pub struct ColorError(pub String);

fn round(v: f64, places: i32) -> f64 {
    let p = 10f64.powi(places);
    (v * p).round() / p
}

fn to_linear(c: f64) -> f64 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn from_linear(c: f64) -> f64 {
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

impl Color {
    pub const BLACK: Color = Color::Oklch { l: 0.0, c: 0.0, h: 0.0, a: 1.0 };
    pub const WHITE: Color = Color::Oklch { l: 1.0, c: 0.0, h: 0.0, a: 1.0 };

    /// From sRGB channels in 0..1.
    pub fn from_srgb(r: f64, g: f64, b: f64, a: f64) -> Color {
        let (r, g, b) = (to_linear(r), to_linear(g), to_linear(b));
        let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
        let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
        let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
        let ll = 0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s;
        let aa = 1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s;
        let bb = 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s;
        let c = (aa * aa + bb * bb).sqrt();
        let h = if c < 1e-4 { 0.0 } else { bb.atan2(aa).to_degrees().rem_euclid(360.0) };
        Color::oklch(ll, c, h, a)
    }

    pub fn oklch(l: f64, c: f64, h: f64, a: f64) -> Color {
        let c = round(c.max(0.0), 4);
        Color::Oklch {
            l: round(l.clamp(0.0, 1.0), 4),
            c,
            h: if c == 0.0 { 0.0 } else { round(h.rem_euclid(360.0), 2) },
            a: round(a.clamp(0.0, 1.0), 3),
        }
    }

    /// sRGB channels in 0..1 (may fall outside 0..1 for wide-gamut colors). None for token refs.
    pub fn to_srgb(&self) -> Option<[f64; 4]> {
        let Color::Oklch { l, c, h, a } = *self else { return None };
        let (aa, bb) = (c * h.to_radians().cos(), c * h.to_radians().sin());
        let l_ = (l + 0.3963377774 * aa + 0.2158037573 * bb).powi(3);
        let m_ = (l - 0.1055613458 * aa - 0.0638541728 * bb).powi(3);
        let s_ = (l - 0.0894841775 * aa - 1.2914855480 * bb).powi(3);
        let r = 4.0767416621 * l_ - 3.3077115913 * m_ + 0.2309699292 * s_;
        let g = -1.2684380046 * l_ + 2.6097574011 * m_ - 0.3413193965 * s_;
        let b = -0.0041960863 * l_ - 0.7034186147 * m_ + 1.7076147010 * s_;
        Some([from_linear(r), from_linear(g), from_linear(b), a])
    }

    pub fn alpha(&self) -> f64 {
        match self {
            Color::Oklch { a, .. } => *a,
            Color::Var(_) => 1.0,
        }
    }

    /// `#rrggbb` / `#rrggbbaa` when the color fits sRGB, else None.
    pub fn to_hex(&self) -> Option<String> {
        let [r, g, b, a] = self.to_srgb()?;
        if [r, g, b].iter().any(|v| !(-0.002..=1.002).contains(v)) {
            return None;
        }
        let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        Some(if a >= 1.0 {
            format!("#{:02x}{:02x}{:02x}", byte(r), byte(g), byte(b))
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", byte(r), byte(g), byte(b), byte(a))
        })
    }

    /// CSS for rendering and export: hex when in sRGB, `oklch()` otherwise, `var()` for tokens.
    pub fn to_css(&self) -> String {
        match self {
            Color::Var(name) => format!("var({name})"),
            c => c.to_hex().unwrap_or_else(|| c.to_string()),
        }
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Color::Var(name) => write!(f, "var({name})"),
            Color::Oklch { l, c, h, a } if *a >= 1.0 => write!(f, "oklch({l} {c} {h})"),
            Color::Oklch { l, c, h, a } => write!(f, "oklch({l} {c} {h} / {a})"),
        }
    }
}

fn number(s: &str, percent_scale: f64) -> Option<f64> {
    let s = s.trim();
    match s.strip_suffix('%') {
        Some(p) => p.parse::<f64>().ok().map(|v| v / 100.0 * percent_scale),
        None if s == "none" => Some(0.0),
        None => s.trim_end_matches("deg").parse().ok(),
    }
}

fn parse_oklch(inner: &str) -> Option<Color> {
    let (main, alpha) = match inner.split_once('/') {
        Some((m, a)) => (m, number(a, 1.0)?),
        None => (inner, 1.0),
    };
    let parts: Vec<&str> = main.split([' ', ',']).filter(|p| !p.is_empty()).collect();
    let [l, c, h] = parts[..] else { return None };
    Some(Color::oklch(number(l, 1.0)?, number(c, 0.4)?, number(h, 1.0)?, alpha))
}

/// The token name inside `var(--name)` or `var(--name, fallback)`.
pub fn var_name(s: &str) -> Option<&str> {
    let inner = s.trim().strip_prefix("var(")?.strip_suffix(')')?;
    let name = inner.split(',').next()?.trim();
    (name.starts_with("--") && name.len() > 2).then_some(name)
}

impl FromStr for Color {
    type Err = ColorError;
    fn from_str(s: &str) -> Result<Color, ColorError> {
        let t = s.trim();
        if let Some(name) = var_name(t) {
            return Ok(Color::Var(name.to_string()));
        }
        let lower = t.to_ascii_lowercase();
        if let Some(inner) = lower.strip_prefix("oklch(").and_then(|r| r.strip_suffix(')')) {
            return parse_oklch(inner).ok_or_else(|| ColorError(s.into()));
        }
        let c = csscolorparser::parse(&lower).map_err(|_| ColorError(s.into()))?;
        Ok(Color::from_srgb(c.r as f64, c.g as f64, c.b as f64, c.a as f64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips_through_oklch() {
        for hex in ["#000000", "#ffffff", "#ff0000", "#3b82f6", "#123456", "#f2f2f2", "#00ff00", "#0000ff"] {
            let c: Color = hex.parse().unwrap();
            assert_eq!(c.to_hex().unwrap(), hex, "{c}");
            let again: Color = c.to_string().parse().unwrap();
            assert_eq!(again, c);
        }
    }

    #[test]
    fn parses_css_color_syntaxes() {
        let red: Color = "red".parse().unwrap();
        assert_eq!(red.to_hex().unwrap(), "#ff0000");
        assert_eq!("rgb(255 0 0)".parse::<Color>().unwrap(), red);
        assert_eq!("hsl(0, 100%, 50%)".parse::<Color>().unwrap(), red);
        let half: Color = "rgba(0,0,0,0.5)".parse().unwrap();
        assert_eq!(half.to_css(), "#00000080");
        assert_eq!("oklch(62.8% 0.2577 29.23)".parse::<Color>().unwrap().to_hex().unwrap(), "#ff0000");
        assert_eq!("var(--color-brand, #fff)".parse::<Color>().unwrap(), Color::Var("--color-brand".into()));
        assert!("nope".parse::<Color>().is_err());
    }

    #[test]
    fn wide_gamut_colors_stay_oklch() {
        let c: Color = "oklch(0.7 0.35 145)".parse().unwrap();
        assert_eq!(c.to_hex(), None);
        assert_eq!(c.to_css(), "oklch(0.7 0.35 145)");
    }
}
