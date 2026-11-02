//! Page names, keys and file name encoding.

use unicode_normalization::UnicodeNormalization;

/// Case-insensitive, NFC-normalized identity of a page.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PageKey(String);

impl PageKey {
    /// NFC-normalizes then lowercases `name`; trims surrounding whitespace.
    pub fn new(name: &str) -> Self {
        let normalized: String = name.trim().nfc().collect();
        Self(normalized.to_lowercase())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Characters that are unsafe in file names and therefore percent-encoded.
const RESERVED: [char; 10] = ['<', '>', ':', '"', '\\', '|', '?', '*', '#', '%'];

/// Encodes a page title into a file stem: `/` → `___`, other characters that
/// are unsafe in file names are percent-encoded.
///
/// Logseq's triple-lowbar format would also leave literal underscores alone,
/// but a literal `___` is indistinguishable from a slash and `a_/b` and
/// `a/_b` would encode to the same stem. To keep the encoding injective, a
/// literal `_` is written as `%5F` when it touches another `_` or a `/`; other
/// underscores stay as they are.
pub fn title_to_file_stem(title: &str) -> String {
    let chars: Vec<char> = title.chars().collect();
    let mut out = String::with_capacity(title.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == '/' {
            out.push_str("___");
            continue;
        }
        if c == '_' {
            let prev = i.checked_sub(1).map(|j| chars[j]);
            let next = chars.get(i + 1).copied();
            let ambiguous = matches!(prev, Some('_' | '/')) || matches!(next, Some('_' | '/'));
            if ambiguous {
                out.push_str("%5F");
            } else {
                out.push('_');
            }
            continue;
        }
        let edge = i == 0 || i + 1 == chars.len();
        let unsafe_name =
            RESERVED.contains(&c) || c.is_control() || (edge && (c == '.' || c == ' '));
        if unsafe_name {
            let mut buf = [0u8; 4];
            for byte in c.encode_utf8(&mut buf).bytes() {
                out.push('%');
                push_upper_hex(byte, &mut out);
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Inverse of [`title_to_file_stem`].
///
/// Invalid or dangling `%XX` sequences are kept verbatim.
pub fn file_stem_to_title(stem: &str) -> String {
    let bytes = stem.as_bytes();
    let mut out = String::with_capacity(stem.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if let Some((c, len)) = decode_percent_char(&bytes[i..]) {
                out.push(c);
                i += len;
                continue;
            }
            out.push('%');
            i += 1;
            continue;
        }
        if bytes[i] == b'_' && bytes.get(i + 1) == Some(&b'_') && bytes.get(i + 2) == Some(&b'_') {
            out.push('/');
            i += 3;
            continue;
        }
        // The stem is a `&str`, so `i` is always a char boundary here.
        let c = stem[i..].chars().next().expect("string has a char");
        out.push(c);
        i += c.len_utf8();
    }
    out
}

fn push_upper_hex(byte: u8, out: &mut String) {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    out.push(char::from(DIGITS[usize::from(byte >> 4)]));
    out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Reads one `%XX` byte at the start of `bytes`.
fn percent_byte(bytes: &[u8]) -> Option<u8> {
    if bytes.first() != Some(&b'%') || bytes.len() < 3 {
        return None;
    }
    let high = hex_value(bytes[1])?;
    let low = hex_value(bytes[2])?;
    Some(high << 4 | low)
}

/// Decodes a run of `%XX` bytes starting at `bytes[0]` into one char.
/// Returns the char and how many input bytes it consumed.
fn decode_percent_char(bytes: &[u8]) -> Option<(char, usize)> {
    let first = percent_byte(bytes)?;
    let width = match first {
        0x00..=0x7f => 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return None,
    };
    let mut buf = [0u8; 4];
    buf[0] = first;
    for (index, slot) in buf.iter_mut().enumerate().take(width).skip(1) {
        *slot = percent_byte(&bytes[index * 3..])?;
    }
    let text = std::str::from_utf8(&buf[..width]).ok()?;
    let c = text.chars().next()?;
    Some((c, width * 3))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn page_key_trims_normalizes_and_lowercases() {
        assert_eq!(PageKey::new("  Hello World  ").as_str(), "hello world");
        assert_eq!(PageKey::new("किताब").as_str(), "किताब");
        assert_eq!(PageKey::new("CAFÉ").as_str(), "café");
        assert_eq!(PageKey::new("中文 页面").as_str(), "中文 页面");
    }

    #[test]
    fn page_key_nfc_normalizes() {
        let decomposed = PageKey::new("Cafe\u{301}");
        let composed = PageKey::new("Caf\u{e9}");
        assert_eq!(decomposed, composed);
        assert_eq!(decomposed.as_str(), "caf\u{e9}");
    }

    #[test]
    fn stem_keeps_plain_characters() {
        assert_eq!(title_to_file_stem("Hello World"), "Hello World");
        assert_eq!(title_to_file_stem("中文 页面"), "中文 页面");
        assert_eq!(title_to_file_stem("emoji 🚀"), "emoji 🚀");
        assert_eq!(title_to_file_stem("a.b"), "a.b");
        assert_eq!(title_to_file_stem("a-b_c"), "a-b_c");
    }

    #[test]
    fn stem_maps_slash_to_triple_lowbar() {
        assert_eq!(title_to_file_stem("a/b"), "a___b");
        assert_eq!(title_to_file_stem("/"), "___");
        assert_eq!(title_to_file_stem("a//b"), "a______b");
        assert_eq!(file_stem_to_title("a___b"), "a/b");
        assert_eq!(file_stem_to_title("a______b"), "a//b");
    }

    #[test]
    fn stem_encodes_reserved_characters() {
        assert_eq!(
            title_to_file_stem(r#"< > : " \ | ? * # %"#),
            "%3C %3E %3A %22 %5C %7C %3F %2A %23 %25"
        );
        assert_eq!(title_to_file_stem("a\tb\nc"), "a%09b%0Ac");
        assert_eq!(title_to_file_stem("a\u{7f}b"), "a%7Fb");
    }

    #[test]
    fn stem_encodes_leading_and_trailing_dots_and_spaces() {
        assert_eq!(title_to_file_stem(".hidden"), "%2Ehidden");
        assert_eq!(title_to_file_stem("hidden."), "hidden%2E");
        assert_eq!(title_to_file_stem(" hidden"), "%20hidden");
        assert_eq!(title_to_file_stem("hidden "), "hidden%20");
        assert_eq!(title_to_file_stem("."), "%2E");
        assert_eq!(title_to_file_stem(" "), "%20");
    }

    #[test]
    fn stem_disambiguates_literal_underscores() {
        assert_eq!(title_to_file_stem("a_b"), "a_b");
        assert_eq!(title_to_file_stem("a__b"), "a%5F%5Fb");
        assert_eq!(title_to_file_stem("a___b"), "a%5F%5F%5Fb");
        assert_eq!(title_to_file_stem("a_/b"), "a%5F___b");
        assert_eq!(title_to_file_stem("a/_b"), "a___%5Fb");
    }

    #[test]
    fn title_decodes_percent_sequences() {
        assert_eq!(file_stem_to_title("%E4%B8%AD"), "中");
        assert_eq!(file_stem_to_title("%41%42"), "AB");
        assert_eq!(file_stem_to_title("%2F"), "/");
        assert_eq!(file_stem_to_title("a%5F___b"), "a_/b");
        assert_eq!(file_stem_to_title("a_b"), "a_b");
        assert_eq!(file_stem_to_title("a__b"), "a__b");
    }

    #[test]
    fn title_keeps_invalid_escapes() {
        assert_eq!(file_stem_to_title("%ZZ"), "%ZZ");
        assert_eq!(file_stem_to_title("%4"), "%4");
        assert_eq!(file_stem_to_title("100%"), "100%");
        assert_eq!(file_stem_to_title("%FF"), "%FF");
        assert_eq!(file_stem_to_title("%E4%B8"), "%E4%B8");
        assert_eq!(file_stem_to_title("%E4%B8%ADx"), "中x");
    }

    proptest! {
        #[test]
        fn stem_roundtrips(chars in proptest::collection::vec(any::<char>(), 0..24)) {
            let title: String = chars.into_iter().filter(|c| *c != '\0').collect();
            prop_assert_eq!(file_stem_to_title(&title_to_file_stem(&title)), title);
        }
    }
}
