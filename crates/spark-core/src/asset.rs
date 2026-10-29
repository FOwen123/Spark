//! Content-addressed assets.

use sha2::{Digest, Sha256};

/// The content hash used as an asset's id: the first 32 hex digits of its SHA-256.
pub fn asset_hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().take(16).map(|b| format!("{b:02x}")).collect()
}

/// The image type of `bytes`, sniffed from its first bytes. Only formats browsers render.
pub fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    let b = bytes;
    if b.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("image/png")
    } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if b.starts_with(b"GIF8") {
        Some("image/gif")
    } else if b.len() > 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("image/webp")
    } else if b.len() > 12 && &b[4..12] == b"ftypavif" {
        Some("image/avif")
    } else {
        let head = String::from_utf8_lossy(&b[..b.len().min(512)]).to_ascii_lowercase();
        (head.contains("<svg")).then_some("image/svg+xml")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_and_sniffs() {
        assert_eq!(asset_hash(b"abc").len(), 32);
        assert_eq!(asset_hash(b"abc"), asset_hash(b"abc"));
        assert_ne!(asset_hash(b"abc"), asset_hash(b"abd"));
        assert_eq!(image_mime(&[0x89, b'P', b'N', b'G', 0, 0]), Some("image/png"));
        assert_eq!(image_mime(b"<?xml version=\"1.0\"?><svg xmlns=\"\"/>"), Some("image/svg+xml"));
        assert_eq!(image_mime(b"hello"), None);
    }
}
