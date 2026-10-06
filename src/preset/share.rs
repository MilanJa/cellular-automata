//! Shareable links: the whole preset (bundle TOML) deflated and base64url-encoded into a URL
//! fragment, `#p=<code>`, so one link reproduces a scene exactly.

use base64::Engine;

use super::Preset;
use super::bundle::{from_bundle, to_bundle};

const FRAGMENT_KEY: &str = "p=";

/// Public web build; the desktop uses it so links copied there open in a browser.
pub const PAGES_URL: &str = "https://milanja.github.io/cellular-automata/";

pub fn encode_share_code(preset: &Preset) -> anyhow::Result<String> {
    let text = to_bundle(preset)?;
    let compressed = miniz_oxide::deflate::compress_to_vec(text.as_bytes(), 9);
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(compressed))
}

pub fn decode_share_code(code: &str) -> anyhow::Result<Preset> {
    anyhow::ensure!(!code.is_empty(), "empty share code");
    let compressed = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(code.trim())
        .map_err(|e| anyhow::anyhow!("share code is not valid base64url: {e}"))?;
    let bytes = miniz_oxide::inflate::decompress_to_vec(&compressed)
        .map_err(|e| anyhow::anyhow!("share code did not decompress: {e:?}"))?;
    let text = String::from_utf8(bytes)?;
    from_bundle(&text)
}

/// The code from a URL fragment such as `#p=…` (leading `#` optional).
pub fn share_code_from_fragment(fragment: &str) -> Option<String> {
    let f = fragment.strip_prefix('#').unwrap_or(fragment);
    f.split('&').find_map(|part| part.strip_prefix(FRAGMENT_KEY)).filter(|c| !c.is_empty()).map(str::to_string)
}

/// `base` with its fragment replaced by `#p=<code>`.
pub fn share_url(base: &str, code: &str) -> String {
    let base = base.split('#').next().unwrap_or(base);
    format!("{base}#{FRAGMENT_KEY}{code}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::builtin::{BUILTINS, load_builtin};

    #[test]
    fn a_preset_round_trips_through_a_link_code() {
        let p = load_builtin(&BUILTINS[5]); // Neon Life: long shaders, params, a modulation
        let code = encode_share_code(&p).unwrap();
        assert!(code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'), "url-safe");
        assert!(code.len() < 4000, "compressed code should stay well below URL limits: {}", code.len());
        let back = decode_share_code(&code).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn garbage_codes_are_rejected() {
        assert!(decode_share_code("not base64!!").is_err());
        assert!(decode_share_code("AAAA").is_err());
        assert!(decode_share_code("").is_err());
    }

    #[test]
    fn share_fragment_is_extracted_from_a_url_fragment() {
        assert_eq!(share_code_from_fragment("#p=abc_-9"), Some("abc_-9".to_string()));
        assert_eq!(share_code_from_fragment("p=abc"), Some("abc".to_string()));
        assert_eq!(share_code_from_fragment("#x=1"), None);
        assert_eq!(share_code_from_fragment(""), None);
    }

    #[test]
    fn share_url_is_built_from_a_base() {
        assert_eq!(share_url("https://example.org/app/", "c0de"), "https://example.org/app/#p=c0de");
        assert_eq!(share_url("https://example.org/app/#p=old", "new"), "https://example.org/app/#p=new");
    }
}
