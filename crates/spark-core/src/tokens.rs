//! Design tokens as CSS custom properties in Tailwind v4 theme namespaces.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::color::var_name;

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Token {
    /// Full custom property name, e.g. `--color-brand`.
    pub name: String,
    /// CSS value; may alias another token with `var(--x)`.
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Namespace prefix → token type. Longer prefixes first so `--font-weight-` wins over `--font-`.
pub const NAMESPACES: &[(&str, &str)] = &[
    ("--font-weight", "fontWeight"),
    ("--breakpoint", "breakpoint"),
    ("--container", "container"),
    ("--tracking", "letterSpacing"),
    ("--leading", "lineHeight"),
    ("--opacity", "opacity"),
    ("--spacing", "spacing"),
    ("--radius", "radius"),
    ("--color", "color"),
    ("--font", "fontFamily"),
    ("--text", "fontSize"),
];

/// The token type for a name, or None if it isn't in a known namespace.
pub fn token_type(name: &str) -> Option<&'static str> {
    NAMESPACES.iter().find_map(|(ns, ty)| {
        let rest = name.strip_prefix(ns)?;
        (rest.is_empty() || (rest.len() > 1 && rest.starts_with('-'))).then_some(*ty)
    })
}

pub fn validate_name(name: &str) -> Result<(), String> {
    let body = name.strip_prefix("--").ok_or_else(|| format!("token `{name}` must start with --"))?;
    if body.is_empty() || !body.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(format!("token `{name}` may only use letters, digits, - and _"));
    }
    if token_type(name).is_none() {
        let list: Vec<_> = NAMESPACES.iter().map(|(ns, _)| format!("{ns}-*")).collect();
        return Err(format!("token `{name}` is not in a theme namespace ({})", list.join(", ")));
    }
    Ok(())
}

/// Resolves aliases to a final value. None for unknown names or alias cycles.
pub fn resolve(tokens: &BTreeMap<String, Token>, name: &str) -> Option<String> {
    let mut current = name;
    for _ in 0..32 {
        let value = &tokens.get(current)?.value;
        match var_name(value) {
            Some(next) => current = next,
            None => return Some(value.clone()),
        }
    }
    None
}

/// `@theme { ... }` (Tailwind v4) or `:root { ... }` CSS.
pub fn to_css(tokens: &BTreeMap<String, Token>, theme: bool) -> String {
    let mut out = String::from(if theme { "@theme {\n" } else { ":root {\n" });
    for t in tokens.values() {
        if let Some(d) = &t.description {
            out.push_str(&format!("  /* {} */\n", d.replace("*/", "* /")));
        }
        out.push_str(&format!("  {}: {};\n", t.name, t.value));
    }
    out.push_str("}\n");
    out
}

/// FNV-1a 32-bit hash of the token set, as 8 hex digits. The empty set hashes to `811c9dc5`.
pub fn tokens_hash(tokens: &BTreeMap<String, Token>) -> String {
    let mut h: u32 = 0x811c9dc5;
    for t in tokens.values() {
        for b in t.name.bytes().chain(*b":").chain(t.value.bytes()).chain(*b";") {
            h ^= b as u32;
            h = h.wrapping_mul(0x01000193);
        }
    }
    format!("{h:08x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(pairs: &[(&str, &str)]) -> BTreeMap<String, Token> {
        pairs
            .iter()
            .map(|(n, v)| (n.to_string(), Token { name: n.to_string(), value: v.to_string(), description: None }))
            .collect()
    }

    #[test]
    fn namespaces() {
        assert_eq!(token_type("--color-brand-500"), Some("color"));
        assert_eq!(token_type("--font-weight-bold"), Some("fontWeight"));
        assert_eq!(token_type("--font-display"), Some("fontFamily"));
        assert_eq!(token_type("--spacing"), Some("spacing"));
        assert_eq!(token_type("--colors-x"), None);
        assert!(validate_name("--radius-lg").is_ok());
        assert!(validate_name("--banana").is_err());
        assert!(validate_name("color-x").is_err());
    }

    #[test]
    fn aliases_resolve_and_cycles_fail() {
        let t = set(&[
            ("--color-a", "var(--color-b)"),
            ("--color-b", "#fff"),
            ("--color-x", "var(--color-y)"),
            ("--color-y", "var(--color-x)"),
        ]);
        assert_eq!(resolve(&t, "--color-a").as_deref(), Some("#fff"));
        assert_eq!(resolve(&t, "--color-x"), None);
        assert_eq!(resolve(&t, "--color-missing"), None);
    }

    #[test]
    fn css_export_and_hash() {
        let t = set(&[("--color-brand", "#000"), ("--spacing-4", "16px")]);
        assert_eq!(to_css(&t, true), "@theme {\n  --color-brand: #000;\n  --spacing-4: 16px;\n}\n");
        assert!(to_css(&t, false).starts_with(":root {"));
        assert_eq!(tokens_hash(&BTreeMap::new()), "811c9dc5");
        assert_ne!(tokens_hash(&t), tokens_hash(&set(&[("--color-brand", "#111")])));
    }
}
