//! The style model: a typed subset of CSS for layout plus a structured design layer
//! (fills, border, radius, shadows, effects, typography). Anything Spark doesn't model is kept
//! verbatim in `raw`, and properties toggled off live in `disabled`.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    str::FromStr,
};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::color::{Color, var_name};

pub(crate) fn fmt_num(v: f64) -> String {
    let r = (v * 1000.0).round() / 1000.0;
    if r == 0.0 { "0".into() } else { format!("{r}") }
}

/// A CSS length.
#[derive(Clone, PartialEq, Debug)]
pub enum Len {
    Px(f64),
    Pct(f64),
    Em(f64),
    Rem(f64),
    /// Unitless (line-height multipliers).
    Num(f64),
    Var(String),
}

impl Len {
    pub fn px(&self) -> Option<f64> {
        match self {
            Len::Px(v) => Some(*v),
            _ => None,
        }
    }

    /// Parses a CSS length. Bare numbers are px unless `unitless` is set (line-height).
    pub fn parse(s: &str, unitless: bool) -> Option<Len> {
        let s = s.trim();
        if let Some(name) = var_name(s) {
            return Some(Len::Var(name.into()));
        }
        let num = |t: &str| t.trim().parse::<f64>().ok().filter(|v| v.is_finite());
        if let Some(v) = s.strip_suffix("px") {
            return num(v).map(Len::Px);
        }
        if let Some(v) = s.strip_suffix('%') {
            return num(v).map(Len::Pct);
        }
        if let Some(v) = s.strip_suffix("rem") {
            return num(v).map(Len::Rem);
        }
        if let Some(v) = s.strip_suffix("em") {
            return num(v).map(Len::Em);
        }
        num(s).map(if unitless { Len::Num } else { Len::Px })
    }
}

impl fmt::Display for Len {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Len::Px(v) => write!(f, "{}px", fmt_num(*v)),
            Len::Pct(v) => write!(f, "{}%", fmt_num(*v)),
            Len::Em(v) => write!(f, "{}em", fmt_num(*v)),
            Len::Rem(v) => write!(f, "{}rem", fmt_num(*v)),
            Len::Num(v) => write!(f, "{}", fmt_num(*v)),
            Len::Var(n) => write!(f, "var({n})"),
        }
    }
}

impl Serialize for Len {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Len {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match serde_json::Value::deserialize(d)? {
            serde_json::Value::Number(n) => {
                n.as_f64().map(Len::Px).ok_or_else(|| serde::de::Error::custom("bad number"))
            }
            serde_json::Value::String(s) => {
                Len::parse(&s, true).ok_or_else(|| serde::de::Error::custom(format!("invalid length `{s}`")))
            }
            v => Err(serde::de::Error::custom(format!("invalid length {v}"))),
        }
    }
}

/// Sizing on one axis.
#[derive(Clone, PartialEq, Debug)]
pub enum Size {
    Fixed(Len),
    /// Stretch to fill the parent.
    Fill,
    /// Hug the content.
    Hug,
}

impl fmt::Display for Size {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Size::Fixed(l) => l.fmt(f),
            Size::Fill => f.write_str("fill"),
            Size::Hug => f.write_str("hug"),
        }
    }
}

impl FromStr for Size {
    type Err = ();
    fn from_str(s: &str) -> Result<Size, ()> {
        match s.trim() {
            "fill" | "stretch" | "-webkit-fill-available" => Ok(Size::Fill),
            "hug" | "fit-content" | "max-content" => Ok(Size::Hug),
            other => Len::parse(other, false).map(Size::Fixed).ok_or(()),
        }
    }
}

impl Serialize for Size {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Size {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match serde_json::Value::deserialize(d)? {
            serde_json::Value::Number(n) => {
                n.as_f64().map(|v| Size::Fixed(Len::Px(v))).ok_or_else(|| serde::de::Error::custom("bad number"))
            }
            serde_json::Value::String(s) => {
                s.parse().map_err(|_| serde::de::Error::custom(format!("invalid size `{s}`")))
            }
            v => Err(serde::de::Error::custom(format!("invalid size {v}"))),
        }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Stop {
    pub color: Color,
    /// Position along the gradient, 0..1.
    pub pos: f64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Fit {
    #[default]
    Cover,
    Contain,
    Fill,
    Tile,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum Fill {
    Solid {
        color: Color,
        #[serde(default, skip_serializing_if = "is_false")]
        hidden: bool,
    },
    Linear {
        /// CSS gradient angle in degrees (180 = top to bottom).
        angle: f64,
        stops: Vec<Stop>,
        #[serde(default, skip_serializing_if = "is_false")]
        hidden: bool,
    },
    Radial {
        stops: Vec<Stop>,
        #[serde(default, skip_serializing_if = "is_false")]
        hidden: bool,
    },
    Image {
        /// Content hash of the image asset.
        asset: String,
        #[serde(default)]
        fit: Fit,
        #[serde(default, skip_serializing_if = "is_false")]
        hidden: bool,
    },
}

impl Fill {
    pub fn hidden(&self) -> bool {
        match self {
            Fill::Solid { hidden, .. }
            | Fill::Linear { hidden, .. }
            | Fill::Radial { hidden, .. }
            | Fill::Image { hidden, .. } => *hidden,
        }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Border {
    /// Top, right, bottom, left widths in px.
    pub width: [f64; 4],
    pub style: String,
    pub color: Color,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shadow {
    #[serde(default, skip_serializing_if = "is_false")]
    pub inset: bool,
    pub x: f64,
    pub y: f64,
    pub blur: f64,
    #[serde(default)]
    pub spread: f64,
    pub color: Color,
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden: bool,
}

/// A node's style. Field names are camelCase in JSON and match `Set` property paths (`style.<field>`).
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Style {
    // Box and position
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<Size>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<Size>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_width: Option<Len>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_width: Option<Len>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_height: Option<Len>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_height: Option<Len>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left: Option<Len>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top: Option<Len>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub right: Option<Len>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bottom: Option<Len>,
    /// Degrees.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub z_index: Option<i64>,
    // Layout
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flex_direction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flex_wrap: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub align_items: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub justify_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub align_self: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gap: Option<Len>,
    /// Top, right, bottom, left.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub padding: Option<[Len; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flex_grow: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flex_shrink: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flex_basis: Option<Len>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overflow: Option<String>,
    // Appearance
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blend: Option<String>,
    /// Top-left, top-right, bottom-right, bottom-left.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius: Option<[Len; 4]>,
    /// Top-most first, like CSS background layers.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fills: Vec<Fill>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border: Option<Border>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub shadows: Vec<Shadow>,
    /// Layer blur radius in px.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blur: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backdrop_blur: Option<f64>,
    // Typography
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size: Option<Len>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_weight: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_style: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_height: Option<Len>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub letter_spacing: Option<Len>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_align: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_decoration: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_transform: Option<String>,
    /// Imported CSS Spark doesn't model, by kebab-case property name.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub raw: BTreeMap<String, String>,
    /// Properties switched off but kept (kebab-case CSS names).
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub disabled: BTreeSet<String>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum StyleError {
    #[error("unknown style field `{0}`")]
    UnknownField(String),
    #[error("invalid value for `{0}`: {1}")]
    Invalid(String, String),
}

impl Style {
    /// Sets one field by its JSON name (`null` clears it) and returns the previous value.
    pub fn set_field(&mut self, field: &str, value: serde_json::Value) -> Result<serde_json::Value, StyleError> {
        let serde_json::Value::Object(mut obj) = serde_json::to_value(&*self).expect("style serializes") else {
            unreachable!()
        };
        if !STYLE_FIELDS.contains(&field) {
            return Err(StyleError::UnknownField(field.into()));
        }
        let old = obj.remove(field).unwrap_or(serde_json::Value::Null);
        if !value.is_null() {
            obj.insert(field.into(), value);
        }
        *self = serde_json::from_value(serde_json::Value::Object(obj))
            .map_err(|e| StyleError::Invalid(field.into(), e.to_string()))?;
        Ok(old)
    }

    pub fn get_field(&self, field: &str) -> serde_json::Value {
        serde_json::to_value(self).ok().and_then(|v| v.get(field).cloned()).unwrap_or(serde_json::Value::Null)
    }

    pub fn is_flex(&self) -> bool {
        matches!(self.display.as_deref(), Some("flex" | "inline-flex"))
    }

    pub fn is_row(&self) -> bool {
        self.is_flex() && !self.flex_direction.as_deref().unwrap_or("row").starts_with("column")
    }
}

pub const STYLE_FIELDS: &[&str] = &[
    "width",
    "height",
    "minWidth",
    "maxWidth",
    "minHeight",
    "maxHeight",
    "position",
    "left",
    "top",
    "right",
    "bottom",
    "rotate",
    "zIndex",
    "display",
    "flexDirection",
    "flexWrap",
    "alignItems",
    "justifyContent",
    "alignSelf",
    "gap",
    "padding",
    "flexGrow",
    "flexShrink",
    "flexBasis",
    "overflow",
    "opacity",
    "blend",
    "radius",
    "fills",
    "border",
    "shadows",
    "blur",
    "backdropBlur",
    "fontFamily",
    "fontSize",
    "fontWeight",
    "fontStyle",
    "lineHeight",
    "letterSpacing",
    "textAlign",
    "color",
    "textDecoration",
    "textTransform",
    "raw",
    "disabled",
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lengths_parse_and_print() {
        assert_eq!(Len::parse("12", false), Some(Len::Px(12.0)));
        assert_eq!(Len::parse("1.5", true), Some(Len::Num(1.5)));
        assert_eq!(Len::parse("-0.02em", false), Some(Len::Em(-0.02)));
        assert_eq!(Len::parse("50%", false).unwrap().to_string(), "50%");
        assert_eq!(Len::parse("var(--spacing-4)", false), Some(Len::Var("--spacing-4".into())));
        assert_eq!(Len::Px(12.0).to_string(), "12px");
        assert_eq!(Len::parse("abc", false), None);
        assert_eq!("fit-content".parse::<Size>(), Ok(Size::Hug));
    }

    #[test]
    fn set_field_validates_and_returns_old_value() {
        let mut s = Style::default();
        assert_eq!(s.set_field("width", json!("120px")).unwrap(), json!(null));
        assert_eq!(s.width, Some(Size::Fixed(Len::Px(120.0))));
        assert_eq!(s.set_field("width", json!("hug")).unwrap(), json!("120px"));
        assert_eq!(s.set_field("width", json!(null)).unwrap(), json!("hug"));
        assert_eq!(s.width, None);
        assert!(matches!(s.set_field("bogus", json!(1)), Err(StyleError::UnknownField(_))));
        assert!(matches!(s.set_field("width", json!("banana")), Err(StyleError::Invalid(..))));
        s.set_field("fills", json!([{"type": "solid", "color": "#ff0000"}])).unwrap();
        assert!(matches!(&s.fills[0], Fill::Solid { color, .. } if color.to_hex().unwrap() == "#ff0000"));
    }

    #[test]
    fn style_fields_list_matches_serde() {
        let mut s = Style::default();
        for f in STYLE_FIELDS {
            s.set_field(f, serde_json::Value::Null).unwrap();
        }
    }
}
