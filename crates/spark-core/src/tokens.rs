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
    /// Values for named themes (`dark`, `compact`, …) that differ from `value`. A layer with a
    /// theme, and everything inside it, uses these.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub themes: BTreeMap<String, String>,
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

/// Theme names are lowercase words joined by `-`, so they work as `data-theme` values and CSS
/// selectors. `default` means the base values and can't be a theme.
pub fn validate_theme(name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name.len() <= 40
        && name.split('-').all(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
    if !ok || name == "default" {
        return Err(format!("theme `{name}` must be lowercase letters and digits joined by - (and not `default`)"));
    }
    Ok(())
}

/// Starter tokens: shadcn/ui's default "neutral" color theme (MIT licensed), the most widely
/// copied set of semantic UI colors, with its `dark` theme, named in Tailwind's `--color-*`
/// namespace the way its Tailwind v4 setup maps them, plus its base radius.
pub fn starter() -> Vec<Token> {
    const COLORS: &[(&str, &str, &str)] = &[
        ("background", "oklch(1 0 0)", "oklch(0.145 0 0)"),
        ("foreground", "oklch(0.145 0 0)", "oklch(0.985 0 0)"),
        ("card", "oklch(1 0 0)", "oklch(0.205 0 0)"),
        ("card-foreground", "oklch(0.145 0 0)", "oklch(0.985 0 0)"),
        ("popover", "oklch(1 0 0)", "oklch(0.205 0 0)"),
        ("popover-foreground", "oklch(0.145 0 0)", "oklch(0.985 0 0)"),
        ("primary", "oklch(0.205 0 0)", "oklch(0.922 0 0)"),
        ("primary-foreground", "oklch(0.985 0 0)", "oklch(0.205 0 0)"),
        ("secondary", "oklch(0.97 0 0)", "oklch(0.269 0 0)"),
        ("secondary-foreground", "oklch(0.205 0 0)", "oklch(0.985 0 0)"),
        ("muted", "oklch(0.97 0 0)", "oklch(0.269 0 0)"),
        ("muted-foreground", "oklch(0.556 0 0)", "oklch(0.708 0 0)"),
        ("accent", "oklch(0.97 0 0)", "oklch(0.269 0 0)"),
        ("accent-foreground", "oklch(0.205 0 0)", "oklch(0.985 0 0)"),
        ("destructive", "oklch(0.577 0.245 27.325)", "oklch(0.704 0.191 22.216)"),
        ("border", "oklch(0.922 0 0)", "oklch(1 0 0 / 10%)"),
        ("input", "oklch(0.922 0 0)", "oklch(1 0 0 / 15%)"),
        ("ring", "oklch(0.708 0 0)", "oklch(0.556 0 0)"),
    ];
    let mut out: Vec<Token> = COLORS
        .iter()
        .map(|(name, light, dark)| Token {
            name: format!("--color-{name}"),
            value: light.to_string(),
            description: None,
            themes: BTreeMap::from([("dark".to_string(), dark.to_string())]),
        })
        .collect();
    out.push(Token { name: "--radius".into(), value: "0.625rem".into(), description: None, themes: BTreeMap::new() });
    out
}

/// Every valid theme some token has a value for. Files can hold any name, so invalid ones are left
/// out of everything built from this list.
pub fn theme_names(tokens: &BTreeMap<String, Token>) -> std::collections::BTreeSet<String> {
    tokens.values().flat_map(|t| t.themes.keys()).filter(|n| validate_theme(n).is_ok()).cloned().collect()
}

/// Resolves aliases to a final value. None for unknown names or alias cycles.
pub fn resolve(tokens: &BTreeMap<String, Token>, name: &str) -> Option<String> {
    resolve_in(tokens, name, None)
}

/// Like `resolve`, using a theme's values where it has them.
pub fn resolve_in(tokens: &BTreeMap<String, Token>, name: &str, theme: Option<&str>) -> Option<String> {
    let mut current = name;
    for _ in 0..32 {
        let t = tokens.get(current)?;
        let value = theme.and_then(|th| t.themes.get(th)).unwrap_or(&t.value);
        match var_name(value) {
            Some(next) => current = next,
            None => return Some(value.clone()),
        }
    }
    None
}

/// `@theme { ... }` (Tailwind v4) or `:root { ... }` CSS, then one `[data-theme="<name>"]` block
/// per theme. Each block declares every token (the theme's value or the base one), so a theme
/// nested in another doesn't inherit the outer one's values, and aliases resolve against the
/// block's values (a custom property's `var()` resolves where it is declared).
pub fn to_css(tokens: &BTreeMap<String, Token>, theme: bool) -> String {
    let mut out = String::from(if theme { "@theme {\n" } else { ":root {\n" });
    for t in tokens.values() {
        if let Some(d) = &t.description {
            out.push_str(&format!("  /* {} */\n", d.replace("*/", "* /")));
        }
        out.push_str(&format!("  {}: {};\n", t.name, t.value));
    }
    out.push_str("}\n");
    for name in theme_names(tokens) {
        out.push_str(&format!("\n[data-theme=\"{name}\"] {{\n"));
        for t in tokens.values() {
            out.push_str(&format!("  {}: {};\n", t.name, t.themes.get(&name).unwrap_or(&t.value)));
        }
        out.push_str("}\n");
    }
    out
}

/// FNV-1a 32-bit hash of the token set, as 8 hex digits. The empty set hashes to `811c9dc5`.
pub fn tokens_hash(tokens: &BTreeMap<String, Token>) -> String {
    let mut h: u32 = 0x811c9dc5;
    let mut add = |bytes: &[u8]| {
        for b in bytes {
            h ^= *b as u32;
            h = h.wrapping_mul(0x01000193);
        }
    };
    for t in tokens.values() {
        add(t.name.as_bytes());
        add(b":");
        add(t.value.as_bytes());
        add(b";");
        // Theme values count too; a token without them hashes as before.
        for (theme, v) in &t.themes {
            add(theme.as_bytes());
            add(b"=");
            add(v.as_bytes());
            add(b";");
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
            .map(|(n, v)| {
                (
                    n.to_string(),
                    Token { name: n.to_string(), value: v.to_string(), description: None, themes: BTreeMap::new() },
                )
            })
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

    #[test]
    fn themes_override_values_in_css_and_resolution() {
        let mut t = set(&[("--color-bg", "#fff"), ("--color-card", "var(--color-bg)"), ("--color-ink", "#111")]);
        let plain = tokens_hash(&t);
        t.get_mut("--color-bg").unwrap().themes.insert("dark".into(), "#000".into());
        assert_ne!(tokens_hash(&t), plain);
        assert_eq!(resolve(&t, "--color-card").as_deref(), Some("#fff"));
        assert_eq!(resolve_in(&t, "--color-card", Some("dark")).as_deref(), Some("#000"));
        assert_eq!(
            resolve_in(&t, "--color-ink", Some("dark")).as_deref(),
            Some("#111"),
            "falls back to the base value"
        );
        assert!(
            to_css(&t, true)
                .ends_with("}\n\n[data-theme=\"dark\"] {\n  --color-bg: #000;\n  --color-card: var(--color-bg);\n  --color-ink: #111;\n}\n")
        );
        t.get_mut("--color-ink").unwrap().themes.insert("x\"]{}".into(), "#000".into());
        assert_eq!(theme_names(&t).into_iter().collect::<Vec<_>>(), ["dark"], "names loaded from a file are checked");
        assert!(validate_theme("brand-b2").is_ok());
        for bad in ["", "Dark", "default", "a--b", "-a", "a b", "a\"]"] {
            assert!(validate_theme(bad).is_err(), "{bad}");
        }
    }
}
