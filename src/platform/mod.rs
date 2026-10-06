//! Everything that differs between the desktop and the browser: where saved presets live,
//! how a bundle file leaves or enters the app, and where the startup preset name comes from.
//!
//! The desktop keeps presets as folders under `./presets` and uses native file dialogs; the
//! browser keeps them in `localStorage` and uses download / file-input for export and import.

use crate::preset::slug;

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::*;

#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
pub use web::*;

/// A file dropped on the window (`name`, bytes), or a message saying why it could not be read.
pub type DroppedFile = Result<(String, Vec<u8>), String>;

/// Where a saved preset lives: a folder on disk or a `localStorage` key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SavedLocation {
    Folder(std::path::PathBuf),
    Browser(String),
}

impl SavedLocation {
    pub fn describe(&self) -> String {
        match self {
            SavedLocation::Folder(p) => p.display().to_string(),
            SavedLocation::Browser(key) => format!("browser storage ({})", name_from_storage_key(key).unwrap_or(key)),
        }
    }
}

const KEY_PREFIX: &str = "ca.preset.";

/// `localStorage` key for a preset name.
pub fn storage_key(name: &str) -> String {
    format!("{KEY_PREFIX}{}", slug(name))
}

/// The slug part of a storage key, or `None` for keys that are not ours.
pub fn name_from_storage_key(key: &str) -> Option<&str> {
    key.strip_prefix(KEY_PREFIX)
}

/// Extracts and percent-decodes the `preset` parameter from a URL query string (`?a=1&preset=x`).
pub fn preset_from_query(query: &str) -> Option<String> {
    let query = query.strip_prefix('?').unwrap_or(query);
    query
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| *k == "preset")
        .map(|(_, v)| percent_decode(v))
        .filter(|v| !v.is_empty())
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => match u8::from_str_radix(&s[i + 1..i + 3], 16) {
                Ok(b) => {
                    out.push(b);
                    i += 2;
                }
                Err(_) => out.push(b'%'),
            },
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_keys_are_namespaced_slugs() {
        assert_eq!(storage_key("Game of Life"), "ca.preset.game_of_life");
        assert_eq!(name_from_storage_key("ca.preset.game_of_life"), Some("game_of_life"));
        assert_eq!(name_from_storage_key("other.key"), None);
    }

    #[test]
    fn preset_query_parameter_is_extracted_and_decoded() {
        assert_eq!(preset_from_query("?preset=life"), Some("life".to_string()));
        assert_eq!(preset_from_query("?a=1&preset=gray_scott&b=2"), Some("gray_scott".to_string()));
        assert_eq!(preset_from_query("?preset=my%20thing"), Some("my thing".to_string()));
        assert_eq!(preset_from_query("?preset=a+b"), Some("a b".to_string()));
        assert_eq!(preset_from_query("?other=1"), None);
        assert_eq!(preset_from_query(""), None);
    }
}
