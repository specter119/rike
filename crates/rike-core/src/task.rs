//! Task markers at the start of a block, as Logseq writes them.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    Todo,
    Doing,
    Now,
    Later,
    Wait,
    Done,
    Canceled,
}

impl Marker {
    /// The keyword as written in the file, without the trailing space.
    pub fn keyword(self) -> &'static str {
        match self {
            Marker::Todo => "TODO",
            Marker::Doing => "DOING",
            Marker::Now => "NOW",
            Marker::Later => "LATER",
            Marker::Wait => "WAIT",
            Marker::Done => "DONE",
            Marker::Canceled => "CANCELED",
        }
    }

    /// Open tasks the checkbox can complete.
    pub fn is_open(self) -> bool {
        !matches!(self, Marker::Done | Marker::Canceled)
    }
}

const KEYWORDS: [(&str, Marker); 8] = [
    ("TODO ", Marker::Todo),
    ("DOING ", Marker::Doing),
    ("NOW ", Marker::Now),
    ("LATER ", Marker::Later),
    ("WAIT ", Marker::Wait),
    ("DONE ", Marker::Done),
    ("CANCELED ", Marker::Canceled),
    ("CANCELLED ", Marker::Canceled),
];

/// Splits `text` into its marker and the rest of the block.
pub fn parse_marker(text: &str) -> Option<(Marker, &str)> {
    KEYWORDS
        .iter()
        .find_map(|(prefix, marker)| text.strip_prefix(prefix).map(|rest| (*marker, rest)))
}

/// The text with its marker replaced by `to` (or removed for `None`).
pub fn with_marker(text: &str, to: Option<Marker>) -> String {
    let body = parse_marker(text).map_or(text, |(_, rest)| rest);
    match to {
        Some(m) => format!("{} {body}", m.keyword()),
        None => body.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_keyword() {
        assert_eq!(parse_marker("TODO a"), Some((Marker::Todo, "a")));
        assert_eq!(parse_marker("LATER a b"), Some((Marker::Later, "a b")));
        assert_eq!(parse_marker("CANCELLED x"), Some((Marker::Canceled, "x")));
        assert_eq!(parse_marker("TODOS a"), None);
        assert_eq!(parse_marker("TODO"), None);
        assert_eq!(parse_marker("plain"), None);
    }

    #[test]
    fn replaces_and_removes_markers() {
        assert_eq!(with_marker("LATER x", Some(Marker::Done)), "DONE x");
        assert_eq!(with_marker("x", Some(Marker::Todo)), "TODO x");
        assert_eq!(with_marker("DONE x", None), "x");
    }
}
