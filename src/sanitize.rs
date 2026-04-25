//! Strips terminal escape sequences and control characters from output.

use std::borrow::Cow;

const CSI_MAX_LEN: u16 = 256;
const STRING_SEQ_MAX_LEN: u16 = 4096;

/// Strip control characters and ANSI escape sequences, preserving printable text.
///
/// Returns `Cow::Borrowed` when the input is already clean (no allocation).
#[must_use]
pub fn sanitize(input: &str) -> Cow<'_, str> {
    let needs_sanitize = input
        .bytes()
        .any(|b| (b < b' ' && b != b'\t' && b != b'\n') || b == 0x7f || (0x80..=0x9f).contains(&b));
    if !needs_sanitize {
        return Cow::Borrowed(input);
    }

    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        match c {
            '\t' | '\n' | ' '..='~' => out.push(c),
            '\x1b' | '\u{009b}' => consume_escape_sequence(&mut chars, c == '\u{009b}'),
            c if c > '\x7f' && !('\u{0080}'..='\u{009f}').contains(&c) && !is_bidi_control(c) => out.push(c),
            _ => {}
        }
    }
    Cow::Owned(out)
}

fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200F}' | '\u{200E}')
}

fn consume_csi(chars: &mut std::str::Chars<'_>) {
    let mut n = 0u16;
    for c in chars.by_ref() {
        n += 1;
        if ('\x40'..='~').contains(&c) || n >= CSI_MAX_LEN {
            break;
        }
    }
}

fn consume_escape_sequence(chars: &mut std::str::Chars<'_>, c1_csi: bool) {
    if c1_csi {
        consume_csi(chars);
        return;
    }
    let Some(next) = chars.next() else { return };
    match next {
        '[' => consume_csi(chars),
        ']' => {
            // OSC
            let mut n = 0u16;
            for c in chars.by_ref() {
                n += 1;
                if c == '\x07' || n >= STRING_SEQ_MAX_LEN {
                    break;
                }
                if c == '\x1b' {
                    let _ = chars.next();
                    break;
                }
            }
        }
        'P' | '^' | '_' | 'X' => {
            // DCS, PM, APC, SOS
            let mut n = 0u16;
            for c in chars.by_ref() {
                n += 1;
                if n >= STRING_SEQ_MAX_LEN {
                    break;
                }
                if c == '\x1b' {
                    let _ = chars.next();
                    break;
                }
            }
        }
        _ => {} // Two-char escape, drop both
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_plain_text() {
        assert_eq!(sanitize("hello world"), "hello world");
    }

    #[test]
    fn preserves_unicode() {
        assert_eq!(sanitize("日本語テスト"), "日本語テスト");
    }

    #[test]
    fn preserves_tabs_and_newlines() {
        assert_eq!(sanitize("a\tb\nc"), "a\tb\nc");
    }

    #[test]
    fn strips_csi_escape() {
        assert_eq!(sanitize("before\x1b[31mred\x1b[0mafter"), "beforeredafter");
    }

    #[test]
    fn strips_osc_escape() {
        assert_eq!(sanitize("a\x1b]0;title\x07b"), "ab");
    }

    #[test]
    fn strips_osc_with_st() {
        assert_eq!(sanitize("a\x1b]0;title\x1b\\b"), "ab");
    }

    #[test]
    fn strips_dcs_escape() {
        assert_eq!(sanitize("a\x1bPdata\x1b\\b"), "ab");
    }

    #[test]
    fn strips_c1_control_chars() {
        assert_eq!(sanitize("a\u{0080}b\u{009f}c"), "abc");
    }

    #[test]
    fn strips_null_and_control() {
        assert_eq!(sanitize("a\x00b\x01c\x7f"), "abc");
    }

    #[test]
    fn csi_length_limit() {
        // CSI sequence longer than CSI_MAX_LEN should be truncated.
        let long_csi = format!("a\x1b[{}b", "0;".repeat(300));
        let result = sanitize(&long_csi);
        assert!(result.starts_with('a'));
        assert!(!result.contains('\x1b'));
    }

    #[test]
    fn empty_input() {
        assert_eq!(sanitize(""), "");
    }

    #[test]
    fn lone_escape() {
        assert_eq!(sanitize("\x1b"), "");
    }

    #[test]
    fn strips_c1_csi_with_params() {
        // U+009B is C1 CSI, equivalent to ESC [. "31m" is the SGR parameter.
        assert_eq!(sanitize("before\u{009b}31mafter"), "beforeafter");
    }

    #[test]
    fn strips_osc_hyperlink() {
        assert_eq!(sanitize("a\x1b]8;;https://evil.com\x07click\x1b]8;;\x07b"), "aclickb");
    }

    #[test]
    fn strips_bidi_override_characters() {
        // CVE-2021-42574: BiDi overrides can reorder displayed text.
        assert_eq!(sanitize("a\u{202A}b\u{202E}c\u{2066}d\u{200F}e"), "abcde");
    }
}
