//! Identifiers: `<base36 counter>-<site>` for nodes, `p-<base36>` for pages, ULIDs for files.

use std::{fmt, str::FromStr};

const DIGITS: &[u8; 36] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";

pub fn base36(mut n: u64) -> String {
    let mut out = Vec::new();
    loop {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

pub fn parse_base36(s: &str) -> Option<u64> {
    if s.is_empty() || s.len() > 12 {
        return None;
    }
    s.chars().try_fold(0u64, |acc, c| Some(acc * 36 + c.to_digit(36)? as u64))
}

/// A node id: a file-wide counter plus the site (client) that allocated it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct NodeId {
    pub counter: u64,
    pub site: u32,
}

impl NodeId {
    pub const fn new(counter: u64, site: u32) -> Self {
        Self { counter, site }
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", base36(self.counter), base36(self.site as u64))
    }
}

impl FromStr for NodeId {
    type Err = IdError;
    fn from_str(s: &str) -> Result<Self, IdError> {
        let (c, site) = s.split_once('-').ok_or_else(|| IdError(s.into()))?;
        let counter = parse_base36(c).ok_or_else(|| IdError(s.into()))?;
        let site = parse_base36(site).filter(|v| *v <= u32::MAX as u64).ok_or_else(|| IdError(s.into()))?;
        Ok(Self { counter, site: site as u32 })
    }
}

/// A page id, written `p-<base36>`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct PageId(pub u64);

impl fmt::Display for PageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "p-{}", base36(self.0))
    }
}

impl FromStr for PageId {
    type Err = IdError;
    fn from_str(s: &str) -> Result<Self, IdError> {
        s.strip_prefix("p-").and_then(parse_base36).map(PageId).ok_or_else(|| IdError(s.into()))
    }
}

#[derive(Debug, thiserror::Error)]
#[error("invalid id `{0}`")]
pub struct IdError(pub String);

macro_rules! string_serde {
    ($t:ty) => {
        impl serde::Serialize for $t {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.collect_str(self)
            }
        }
        impl<'de> serde::Deserialize<'de> for $t {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = <std::borrow::Cow<'de, str> as serde::Deserialize>::deserialize(d)?;
                s.parse().map_err(serde::de::Error::custom)
            }
        }
    };
}
pub(crate) use string_serde;
string_serde!(NodeId);
string_serde!(PageId);

/// A new file id (ULID, Crockford base32).
pub fn new_file_id() -> String {
    ulid::Ulid::generate().to_string()
}

pub fn is_file_id(s: &str) -> bool {
    s.len() == 26 && ulid::Ulid::from_string(s).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_ids_round_trip() {
        let id = NodeId::new(1822, 0);
        assert_eq!(id.to_string(), "1EM-0");
        assert_eq!("1EM-0".parse::<NodeId>().unwrap(), id);
        assert_eq!("1em-0".parse::<NodeId>().unwrap(), id);
        assert_eq!(NodeId::new(35, 37).to_string(), "Z-11");
        assert!("1EM".parse::<NodeId>().is_err());
        assert!("-0".parse::<NodeId>().is_err());
        assert!("1E!-0".parse::<NodeId>().is_err());
    }

    #[test]
    fn page_ids_and_file_ids() {
        assert_eq!(PageId(36).to_string(), "p-10");
        assert_eq!("p-10".parse::<PageId>().unwrap(), PageId(36));
        assert!("10".parse::<PageId>().is_err());
        let f = new_file_id();
        assert!(is_file_id(&f));
        assert!(!is_file_id("not-a-ulid"));
    }

    #[test]
    fn ids_serialize_as_strings() {
        assert_eq!(serde_json::to_string(&NodeId::new(10, 0)).unwrap(), "\"A-0\"");
        assert_eq!(serde_json::from_str::<PageId>("\"p-1\"").unwrap(), PageId(1));
    }
}
