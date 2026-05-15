use unicode_general_category::{get_general_category, GeneralCategory};
use unicode_segmentation::UnicodeSegmentation;

/// Application User Model ID. Must match the AUMID set on the installed
/// Start menu shortcut (see installer/installer.wxs) for toasts to be
/// attributed to "webp-tray" instead of falling back to a generic shell.
pub const AUMID: &str = "vivlim.WebpTray";

/// Cap a sanitized stem to a reasonable length so we never blow past the
/// 260-char Windows path limit when combined with the parent directory and
/// disambiguation suffix. We cap by *grapheme clusters*, not bytes or chars,
/// so a CJK or emoji stem isn't sliced through a multi-codepoint glyph.
const MAX_STEM_GRAPHEMES: usize = 100;

/// Windows reserved DOS device names (case-insensitive). Cannot exist as
/// filenames regardless of extension — `CON.png` fails to write.
const DOS_RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Sanitize a filename stem (no extension):
///
/// * strips Windows-reserved characters (`< > : " / \ | ? *`)
/// * strips Unicode categories that almost never have a glyph and so render
///   as a "tofu" square: Control (Cc), Format (Cf), Unassigned (Cn),
///   Private Use (Co), Surrogate (Cs)
/// * keeps everything else (CJK, emoji, accented latin, combining marks, etc.)
/// * collapses runs of whitespace and trims leading/trailing whitespace and dots
/// * caps length to ~100 grapheme clusters so we don't exceed MAX_PATH
/// * if the result is empty, returns `"image"` so we never write a nameless file
/// * if the result equals a reserved DOS device name (CON, PRN, ...), prepends `_`
pub fn sanitize_stem(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_was_space = false;
    for ch in input.chars() {
        if is_windows_reserved(ch) {
            continue;
        }
        if is_unrenderable(ch) {
            continue;
        }
        if ch.is_whitespace() {
            // collapse runs of whitespace to a single ASCII space.
            if !last_was_space && !out.is_empty() {
                out.push(' ');
                last_was_space = true;
            }
            continue;
        }
        last_was_space = false;
        out.push(ch);
    }
    // Windows treats trailing dots/spaces as no-ops, which makes them
    // silently lost and breaks "file already exists" comparisons.
    let trimmed: String = out.trim_matches(|c: char| c == ' ' || c == '.').to_string();

    let capped = cap_graphemes(&trimmed, MAX_STEM_GRAPHEMES);

    if capped.is_empty() {
        return "image".to_string();
    }
    if is_dos_reserved(&capped) {
        // Prepend an underscore rather than appending — preserves the original
        // name visually and keeps the would-be-reserved word recognisable.
        return format!("_{capped}");
    }
    capped
}

fn cap_graphemes(s: &str, max: usize) -> String {
    let graphemes: Vec<&str> = s.graphemes(true).collect();
    if graphemes.len() <= max {
        return s.to_string();
    }
    graphemes[..max].concat()
}

fn is_windows_reserved(c: char) -> bool {
    matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
}

/// Characters that almost never have a glyph in any installed font and so
/// show up as the "tofu" square. We use the Unicode general category as the
/// signal — `Cn`/`Co`/`Cs` are not assigned to any printable character, and
/// `Cc` is real control codes.
///
/// Format (`Cf`) is mostly invisible bookkeeping (BOM, bidi marks, ZWSP)
/// that we DO want to strip from filenames — except for two cases that are
/// structurally part of legitimate text:
///   * U+200D ZERO WIDTH JOINER  — required to render emoji sequences like
///     👨‍👩‍👧 (which would otherwise decompose into three separate emoji)
///   * U+200C ZERO WIDTH NON-JOINER — used in Persian, Arabic, and Indic
///     scripts to control ligature formation
///
/// We deliberately do NOT strip every non-ASCII character; CJK, emoji,
/// combining marks, and the like all have glyphs in the standard Windows
/// font fallback chain and should be preserved.
fn is_unrenderable(c: char) -> bool {
    if c == '\u{200D}' || c == '\u{200C}' {
        return false;
    }
    match get_general_category(c) {
        GeneralCategory::Control          // Cc — \r, \n, \t, etc.
        | GeneralCategory::Format         // Cf — BOM, ZWSP, bidi marks (after the whitelist above)
        | GeneralCategory::Surrogate      // Cs — invalid in UTF-8 to begin with
        | GeneralCategory::PrivateUse     // Co — no defined rendering
        | GeneralCategory::Unassigned     // Cn — code point with no character
            => true,
        _ => false,
    }
}

fn is_dos_reserved(s: &str) -> bool {
    let upper = s.to_ascii_uppercase();
    DOS_RESERVED.iter().any(|r| upper == *r)
}

/// Truncate a string to at most `max_chars` grapheme clusters, prepending
/// an ellipsis if truncated. Char-aware so it never panics on multi-byte
/// UTF-8 like the previous byte-slicing implementation did.
pub fn ellipsize_left(s: &str, max_chars: usize) -> String {
    let graphemes: Vec<&str> = s.graphemes(true).collect();
    if graphemes.len() <= max_chars {
        return s.to_string();
    }
    let kept = max_chars.saturating_sub(1);
    let suffix: String = graphemes[graphemes.len() - kept..].concat();
    format!("…{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_normal_text() {
        assert_eq!(sanitize_stem("hello world"), "hello world");
        assert_eq!(sanitize_stem("café"), "café");
    }

    #[test]
    fn keeps_cjk_and_emoji() {
        assert_eq!(sanitize_stem("猫の写真"), "猫の写真");
        assert_eq!(sanitize_stem("party 🎉 time"), "party 🎉 time");
        // Multi-codepoint emoji (grapheme cluster) survives.
        assert_eq!(sanitize_stem("👨‍👩‍👧"), "👨‍👩‍👧");
    }

    #[test]
    fn strips_tofu() {
        assert_eq!(sanitize_stem("foo\u{E000}bar"), "foobar");
        assert_eq!(sanitize_stem("a\u{200B}b"), "ab"); // ZWSP
        assert_eq!(sanitize_stem("name\x01here"), "namehere"); // ASCII control
        assert_eq!(sanitize_stem("BOM\u{FEFF}here"), "BOMhere"); // BOM
    }

    #[test]
    fn preserves_zwj_for_emoji() {
        // ZWJ (U+200D) is in Format category but is structurally required
        // for emoji sequences. Stripping it would split this into three
        // separate emoji.
        let family = "👨\u{200D}👩\u{200D}👧";
        assert_eq!(sanitize_stem(family), family);
    }

    #[test]
    fn preserves_zwnj_for_persian() {
        // ZWNJ (U+200C) controls ligature formation in Persian/Arabic/Indic
        // scripts and must be preserved.
        let s = "می\u{200C}روم";
        assert_eq!(sanitize_stem(s), s);
    }

    #[test]
    fn strips_reserved_windows_chars() {
        assert_eq!(sanitize_stem("foo:bar?baz"), "foobarbaz");
    }

    #[test]
    fn collapses_whitespace_and_trims_dots() {
        assert_eq!(sanitize_stem("  many   spaces  "), "many spaces");
        assert_eq!(sanitize_stem("trailing..."), "trailing");
    }

    #[test]
    fn fully_stripped_falls_back() {
        assert_eq!(sanitize_stem("\u{E000}\u{E001}"), "image");
        assert_eq!(sanitize_stem(""), "image");
    }

    #[test]
    fn handles_dos_reserved() {
        assert_eq!(sanitize_stem("CON"), "_CON");
        assert_eq!(sanitize_stem("con"), "_con");
        assert_eq!(sanitize_stem("LPT3"), "_LPT3");
        // Only exact matches; substring or longer name is fine.
        assert_eq!(sanitize_stem("CONsole"), "CONsole");
    }

    #[test]
    fn caps_long_stems() {
        let long = "a".repeat(500);
        let out = sanitize_stem(&long);
        assert_eq!(out.chars().count(), MAX_STEM_GRAPHEMES);
    }

    #[test]
    fn caps_long_cjk_without_panic() {
        let long: String = "猫".repeat(200);
        let out = sanitize_stem(&long);
        assert_eq!(out.graphemes(true).count(), MAX_STEM_GRAPHEMES);
        assert!(out.chars().all(|c| c == '猫'));
    }

    #[test]
    fn ellipsize_left_handles_unicode() {
        assert_eq!(ellipsize_left("short", 10), "short");
        let s = "猫の写真フォルダー/very/long/path";
        let out = ellipsize_left(s, 10);
        assert!(out.starts_with('…'));
        assert!(out.graphemes(true).count() <= 10);
    }
}
