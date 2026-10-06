//! Minimal WGSL tokenizer and an egui `TextEdit` layouter that colours tokens.

use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Comment,
    Annotation,
    Keyword,
    Type,
    Builtin,
    Number,
    Attribute,
    Ident,
    Punct,
    Whitespace,
}

const KEYWORDS: &[&str] = &[
    "fn",
    "let",
    "var",
    "const",
    "return",
    "if",
    "else",
    "for",
    "while",
    "loop",
    "break",
    "continue",
    "struct",
    "switch",
    "case",
    "default",
    "discard",
    "true",
    "false",
    "override",
    "continuing",
    "alias",
];
const TYPES: &[&str] = &[
    "f32",
    "i32",
    "u32",
    "bool",
    "f16",
    "vec2",
    "vec3",
    "vec4",
    "mat2x2",
    "mat3x3",
    "mat4x4",
    "array",
    "texture_2d",
    "texture_storage_2d",
    "sampler",
    "rgba32float",
    "write",
    "read",
    "read_write",
    "uniform",
    "storage",
];
const BUILTINS: &[&str] = &[
    "textureLoad",
    "textureStore",
    "select",
    "clamp",
    "min",
    "max",
    "abs",
    "floor",
    "ceil",
    "fract",
    "round",
    "sin",
    "cos",
    "tan",
    "atan2",
    "exp",
    "log",
    "pow",
    "sqrt",
    "dot",
    "cross",
    "length",
    "normalize",
    "distance",
    "mix",
    "smoothstep",
    "step",
    "sign",
    "any",
    "all",
    "cell",
    "prev_cell",
    "hash",
    "rand",
    "wrap",
    "noise",
    "alive",
    "prev_alive",
    "neighbours",
    "neighbours4",
    "moore_sum",
    "laplacian",
    "on",
    "off",
    "on_if",
    "gray",
    "rgb",
    "hsv",
    "palette",
    "cell_at",
    "scene",
    "prev",
    "scene_px",
    "neighbours_hex",
    "neighbours_tri",
    "neighbours_tri12",
    "tri_is_up",
    "hex_cell",
    "hex_local",
    "hex_dist",
    "tri_cell",
    "other",
    "other_alive",
];

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Splits `src` into tokens. The token texts concatenate back to exactly `src`.
pub fn tokenize(src: &str) -> Vec<(TokenKind, &str)> {
    let mut out = Vec::new();
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < src.len() {
        let rest = &src[i..];
        let c = rest.chars().next().unwrap();
        let start = i;
        let kind = if rest.starts_with("//") {
            let end = rest.find('\n').unwrap_or(rest.len());
            i += end;
            let text = &src[start..i];
            if text.trim_start_matches('/').trim_start().starts_with("@param") {
                TokenKind::Annotation
            } else {
                TokenKind::Comment
            }
        } else if c.is_whitespace() {
            i += rest.char_indices().find(|(_, ch)| !ch.is_whitespace()).map_or(rest.len(), |(j, _)| j);
            TokenKind::Whitespace
        } else if c == '@' {
            i += 1;
            while i < src.len() && is_ident_char(bytes[i] as char) {
                i += 1;
            }
            TokenKind::Attribute
        } else if c.is_ascii_digit() || (c == '.' && rest[1..].starts_with(|d: char| d.is_ascii_digit())) {
            i += 1;
            while i < src.len() && ((bytes[i] as char).is_ascii_alphanumeric() || bytes[i] == b'.') {
                i += 1;
            }
            TokenKind::Number
        } else if is_ident_start(c) {
            while i < src.len() && is_ident_char(bytes[i] as char) {
                i += 1;
            }
            let word = &src[start..i];
            if KEYWORDS.contains(&word) {
                TokenKind::Keyword
            } else if TYPES.contains(&word) {
                TokenKind::Type
            } else if BUILTINS.contains(&word) {
                TokenKind::Builtin
            } else {
                TokenKind::Ident
            }
        } else {
            i += c.len_utf8();
            TokenKind::Punct
        };
        out.push((kind, &src[start..i]));
    }
    out
}

#[allow(clippy::disallowed_methods, reason = "the syntax palette is defined here")]
fn color(kind: TokenKind, dark: bool) -> Color32 {
    match (kind, dark) {
        (TokenKind::Comment, true) => Color32::from_rgb(110, 120, 110),
        (TokenKind::Comment, false) => Color32::from_rgb(90, 110, 90),
        (TokenKind::Annotation, true) => Color32::from_rgb(230, 170, 90),
        (TokenKind::Annotation, false) => Color32::from_rgb(170, 100, 20),
        (TokenKind::Keyword, true) => Color32::from_rgb(200, 120, 220),
        (TokenKind::Keyword, false) => Color32::from_rgb(140, 40, 160),
        (TokenKind::Type, true) => Color32::from_rgb(120, 200, 220),
        (TokenKind::Type, false) => Color32::from_rgb(20, 120, 150),
        (TokenKind::Builtin, true) => Color32::from_rgb(120, 190, 140),
        (TokenKind::Builtin, false) => Color32::from_rgb(20, 120, 60),
        (TokenKind::Number, true) => Color32::from_rgb(230, 200, 120),
        (TokenKind::Number, false) => Color32::from_rgb(150, 110, 20),
        (TokenKind::Attribute, true) => Color32::from_rgb(220, 150, 150),
        (TokenKind::Attribute, false) => Color32::from_rgb(160, 60, 60),
        (TokenKind::Ident | TokenKind::Punct | TokenKind::Whitespace, true) => Color32::from_rgb(220, 220, 220),
        (TokenKind::Ident | TokenKind::Punct | TokenKind::Whitespace, false) => Color32::from_rgb(30, 30, 30),
    }
}

pub fn highlight_job(src: &str, font_id: FontId, dark: bool) -> LayoutJob {
    let mut job = LayoutJob::default();
    for (kind, text) in tokenize(src) {
        job.append(text, 0.0, TextFormat { font_id: font_id.clone(), color: color(kind, dark), ..Default::default() });
    }
    job
}

#[derive(Default)]
struct Highlighter;

impl egui::cache::ComputerMut<(&str, bool), LayoutJob> for Highlighter {
    fn compute(&mut self, (src, dark): (&str, bool)) -> LayoutJob {
        highlight_job(src, FontId::monospace(13.0), dark)
    }
}

type HighlightCache = egui::cache::FrameCache<LayoutJob, Highlighter>;

/// Builds a layouter closure for `egui::TextEdit::layouter`. Jobs are cached per frame by egui,
/// keyed on the source text, so unchanged text is not re-tokenised.
pub fn layouter<'a>() -> impl FnMut(&egui::Ui, &dyn egui::TextBuffer, f32) -> Arc<egui::Galley> + 'a {
    move |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
        let dark = ui.visuals().dark_mode;
        let mut job = ui.ctx().memory_mut(|m| m.caches.cache::<HighlightCache>().get((buf.as_str(), dark)).clone());
        job.wrap.max_width = wrap_width;
        ui.fonts_mut(|f| f.layout_job(job))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<(TokenKind, &str)> {
        tokenize(src).into_iter().filter(|(k, _)| *k != TokenKind::Whitespace).collect()
    }

    #[test]
    fn tokens_cover_the_whole_source() {
        let src = "fn rule(pos: vec2<u32>) -> vec4<f32> { // hi\n  return vec4<f32>(1.0); }";
        let joined: String = tokenize(src).iter().map(|(_, s)| *s).collect();
        assert_eq!(joined, src);
    }

    #[test]
    fn classifies_keywords_types_builtins_numbers() {
        let t = kinds("let x: f32 = clamp(1.0, 0u, 2);");
        assert_eq!(t[0], (TokenKind::Keyword, "let"));
        assert_eq!(t[1], (TokenKind::Ident, "x"));
        assert_eq!(t[3], (TokenKind::Type, "f32"));
        assert_eq!(t[5], (TokenKind::Builtin, "clamp"));
        assert_eq!(t[7], (TokenKind::Number, "1.0"));
        assert_eq!(t[9], (TokenKind::Number, "0u"));
    }

    #[test]
    fn comments_and_param_annotations() {
        let t = kinds("// plain\n// @param a: f32 = 1\n@compute");
        assert_eq!(t[0], (TokenKind::Comment, "// plain"));
        assert_eq!(t[1], (TokenKind::Annotation, "// @param a: f32 = 1"));
        assert_eq!(t[2], (TokenKind::Attribute, "@compute"));
    }

    #[test]
    fn job_covers_full_text_with_sections() {
        let src = "fn a() {}";
        let job = highlight_job(src, egui::FontId::monospace(12.0), true);
        assert_eq!(job.text, src);
        // egui merges adjacent sections with identical format, so count <= token count.
        assert!(!job.sections.is_empty());
        assert!(job.sections.len() <= tokenize(src).len());
        assert_eq!(job.sections.last().unwrap().byte_range.end, egui::text::ByteIndex(src.len()));
    }

    #[test]
    fn multibyte_whitespace_terminates_and_covers_source() {
        let src = "let\u{a0}x = 1; // caf\u{e9}\u{2003}done\n";
        let joined: String = tokenize(src).iter().map(|(_, s)| *s).collect();
        assert_eq!(joined, src);
    }
}
