//! Journal date ↔ file name ↔ page title mapping.

/// Day of the week, ISO-8601 numbering (Monday = 1 ... Sunday = 7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

const WEEKDAY_FULL: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

const WEEKDAY_SHORT: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

const MONTH_FULL: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

const MONTH_SHORT: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

impl Weekday {
    pub fn iso_number(self) -> u8 {
        self as u8 + 1
    }

    pub fn from_iso(number: u8) -> Option<Self> {
        Some(match number {
            1 => Self::Monday,
            2 => Self::Tuesday,
            3 => Self::Wednesday,
            4 => Self::Thursday,
            5 => Self::Friday,
            6 => Self::Saturday,
            7 => Self::Sunday,
            _ => return None,
        })
    }

    pub fn full_name(self) -> &'static str {
        WEEKDAY_FULL[usize::from(self as u8)]
    }

    pub fn short_name(self) -> &'static str {
        WEEKDAY_SHORT[usize::from(self as u8)]
    }
}

/// Calendar date without a time zone.
///
/// The supported range is 1..=9999 for the year, so that a `yyyy` pattern is
/// always exactly four digits. Use [`JournalDate::new`] to validate a date;
/// the fields are public for cheap construction from trusted sources.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JournalDate {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

impl JournalDate {
    /// Validates a proleptic Gregorian calendar date.
    pub fn new(year: i32, month: u8, day: u8) -> Option<Self> {
        if !(1..=9999).contains(&year) || !(1..=days_in_month(year, month)).contains(&day) {
            return None;
        }
        Some(Self { year, month, day })
    }

    pub fn weekday(self) -> Weekday {
        let days = days_from_civil(self.year, self.month, self.day);
        let iso = (days + 3).rem_euclid(7) as u8 + 1;
        Weekday::from_iso(iso).expect("ISO weekday numbering is 1..=7")
    }

    /// Shifts the date by `days`; `None` when the result leaves the supported range.
    pub fn add_days(self, days: i64) -> Option<Self> {
        let total = days_from_civil(self.year, self.month, self.day).checked_add(days)?;
        let (year, month, day) = civil_from_days(total);
        Self::new(year, month, day)
    }

    pub fn succ(self) -> Option<Self> {
        self.add_days(1)
    }

    pub fn pred(self) -> Option<Self> {
        self.add_days(-1)
    }
}

/// Local calendar date for a Unix timestamp, given the local UTC offset in
/// seconds. The caller supplies the clock and the offset; `core` has no IO.
pub fn today_from_unix(unix_secs: i64, utc_offset_secs: i32) -> Option<JournalDate> {
    let local = unix_secs.checked_add(i64::from(utc_offset_secs))?;
    let (year, month, day) = civil_from_days(local.div_euclid(86_400));
    JournalDate::new(year, month, day)
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Days since 1970-01-01 (Howard Hinnant's civil calendar algorithm).
fn days_from_civil(year: i32, month: u8, day: u8) -> i64 {
    let year = i64::from(year) - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Inverse of [`days_from_civil`].
fn civil_from_days(days: i64) -> (i32, u8, u8) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    let year = year + i64::from(month <= 2);
    (year as i32, month as u8, day as u8)
}

/// Date format patterns, Logseq/Java style (`yyyy`, `MM`, `dd`, `MMM`, `do`, ...).
///
/// Supported tokens: `yyyy`, `yy`, `MMMM`, `MMM`, `MM`, `M`, `dd`, `d`, `do`,
/// `EEEE`, `EEE`, `E`. Other text is literal; `'text'` quotes a literal and
/// `''` is a literal apostrophe. Month and weekday names are English.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalFormat {
    /// Default `yyyy_MM_dd`.
    pub file_name: String,
    /// Default `MMM do, yyyy`.
    pub page_title: String,
}

impl Default for JournalFormat {
    fn default() -> Self {
        Self {
            file_name: "yyyy_MM_dd".into(),
            page_title: "MMM do, yyyy".into(),
        }
    }
}

impl JournalFormat {
    pub fn format_file_stem(&self, date: JournalDate) -> String {
        format_pattern(&self.file_name, date)
    }

    pub fn format_title(&self, date: JournalDate) -> String {
        format_pattern(&self.page_title, date)
    }

    pub fn parse_file_stem(&self, stem: &str) -> Option<JournalDate> {
        parse_pattern(&self.file_name, stem)
    }

    pub fn parse_title(&self, title: &str) -> Option<JournalDate> {
        parse_pattern(&self.page_title, title)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Year4,
    Year2,
    MonthFull,
    MonthShort,
    Month2,
    Month1,
    Day2,
    Day1,
    DayOrdinal,
    WeekdayFull,
    WeekdayShort,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PatternItem {
    Literal(String),
    Field(Field),
}

fn format_pattern(pattern: &str, date: JournalDate) -> String {
    let mut out = String::new();
    for item in tokenize(pattern) {
        match item {
            PatternItem::Literal(text) => out.push_str(&text),
            PatternItem::Field(field) => write_field(field, date, &mut out),
        }
    }
    out
}

fn parse_pattern(pattern: &str, input: &str) -> Option<JournalDate> {
    let items = tokenize(pattern);
    let chars: Vec<char> = input.chars().collect();
    let fields = parse_items(&items, &chars, 0, Fields::default())?;
    JournalDate::new(fields.year?, fields.month?, fields.day?)
}

fn write_field(field: Field, date: JournalDate, out: &mut String) {
    let year = date.year;
    let month = date.month;
    let day = date.day;
    match field {
        Field::Year4 => out.push_str(&format!("{year:04}")),
        Field::Year2 => {
            let short = year.rem_euclid(100);
            out.push_str(&format!("{short:02}"));
        }
        Field::MonthFull => out.push_str(month_name(month, MONTH_FULL)),
        Field::MonthShort => out.push_str(month_name(month, MONTH_SHORT)),
        Field::Month2 => out.push_str(&format!("{month:02}")),
        Field::Month1 => out.push_str(&month.to_string()),
        Field::Day2 => out.push_str(&format!("{day:02}")),
        Field::Day1 => out.push_str(&day.to_string()),
        Field::DayOrdinal => {
            let suffix = ordinal_suffix(day);
            out.push_str(&format!("{day}{suffix}"));
        }
        Field::WeekdayFull => out.push_str(date.weekday().full_name()),
        Field::WeekdayShort => out.push_str(date.weekday().short_name()),
    }
}

fn month_name(month: u8, names: [&'static str; 12]) -> &'static str {
    names
        .get(usize::from(month.wrapping_sub(1)))
        .copied()
        .unwrap_or("")
}

fn ordinal_suffix(day: u8) -> &'static str {
    match (day % 100, day % 10) {
        (11..=13, _) => "th",
        (_, 1) => "st",
        (_, 2) => "nd",
        (_, 3) => "rd",
        _ => "th",
    }
}

fn tokenize(pattern: &str) -> Vec<PatternItem> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut items = Vec::new();
    let mut literal = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            if chars.get(i + 1) == Some(&'\'') {
                literal.push('\'');
                i += 2;
                continue;
            }
            let mut j = i + 1;
            loop {
                match chars.get(j) {
                    None => break,
                    Some(&'\'') if chars.get(j + 1) == Some(&'\'') => {
                        literal.push('\'');
                        j += 2;
                    }
                    Some(&'\'') => {
                        j += 1;
                        break;
                    }
                    Some(&other) => {
                        literal.push(other);
                        j += 1;
                    }
                }
            }
            i = j;
            continue;
        }
        if let Some((field, len)) = match_field(&chars[i..]) {
            if !literal.is_empty() {
                items.push(PatternItem::Literal(std::mem::take(&mut literal)));
            }
            items.push(PatternItem::Field(field));
            i += len;
            continue;
        }
        literal.push(c);
        i += 1;
    }
    if !literal.is_empty() {
        items.push(PatternItem::Literal(literal));
    }
    items
}

fn match_field(chars: &[char]) -> Option<(Field, usize)> {
    let head = *chars.first()?;
    let tail = &chars[1..];
    match head {
        'y' => {
            if repeat(tail, 'y', 3) {
                Some((Field::Year4, 4))
            } else if repeat(tail, 'y', 1) {
                Some((Field::Year2, 2))
            } else {
                None
            }
        }
        'M' => {
            if repeat(tail, 'M', 3) {
                Some((Field::MonthFull, 4))
            } else if repeat(tail, 'M', 2) {
                Some((Field::MonthShort, 3))
            } else if repeat(tail, 'M', 1) {
                Some((Field::Month2, 2))
            } else {
                Some((Field::Month1, 1))
            }
        }
        'd' => {
            if repeat(tail, 'd', 1) {
                Some((Field::Day2, 2))
            } else if tail.first() == Some(&'o') {
                Some((Field::DayOrdinal, 2))
            } else {
                Some((Field::Day1, 1))
            }
        }
        'E' => {
            let extra = tail.iter().take_while(|c| **c == 'E').count();
            if extra >= 3 {
                Some((Field::WeekdayFull, 4))
            } else {
                // `E`, `EE` and `EEE` all render the short weekday name.
                Some((Field::WeekdayShort, 1 + extra.min(2)))
            }
        }
        _ => None,
    }
}

fn repeat(chars: &[char], c: char, count: usize) -> bool {
    chars.len() >= count && chars[..count].iter().all(|x| *x == c)
}

#[derive(Clone, Copy, Default)]
struct Fields {
    year: Option<i32>,
    month: Option<u8>,
    day: Option<u8>,
}

fn parse_items(
    items: &[PatternItem],
    input: &[char],
    pos: usize,
    fields: Fields,
) -> Option<Fields> {
    let Some((item, rest)) = items.split_first() else {
        return (pos == input.len()).then_some(fields);
    };
    match item {
        PatternItem::Literal(text) => {
            let lit: Vec<char> = text.chars().collect();
            if input.get(pos..pos + lit.len()) == Some(lit.as_slice()) {
                parse_items(rest, input, pos + lit.len(), fields)
            } else {
                None
            }
        }
        PatternItem::Field(field) => parse_field(*field, rest, input, pos, fields),
    }
}

fn parse_field(
    field: Field,
    rest: &[PatternItem],
    input: &[char],
    pos: usize,
    fields: Fields,
) -> Option<Fields> {
    match field {
        Field::Year4 => parse_number(rest, input, pos, fields, 4, 4, |fields, value| {
            (1..=9999).contains(&value).then_some(Fields {
                year: Some(value as i32),
                ..fields
            })
        }),
        Field::Year2 => parse_number(rest, input, pos, fields, 2, 2, |fields, value| {
            Some(Fields {
                year: Some(2000 + value as i32),
                ..fields
            })
        }),
        Field::MonthFull | Field::MonthShort => {
            let (month, len) = match_month(input, pos)?;
            let next = Fields {
                month: Some(month),
                ..fields
            };
            parse_items(rest, input, pos + len, next)
        }
        Field::Month2 => parse_number(rest, input, pos, fields, 2, 2, |fields, value| {
            (1..=12).contains(&value).then_some(Fields {
                month: Some(value as u8),
                ..fields
            })
        }),
        Field::Month1 => parse_number(rest, input, pos, fields, 1, 2, |fields, value| {
            (1..=12).contains(&value).then_some(Fields {
                month: Some(value as u8),
                ..fields
            })
        }),
        Field::Day2 => parse_number(rest, input, pos, fields, 2, 2, |fields, value| {
            (1..=31).contains(&value).then_some(Fields {
                day: Some(value as u8),
                ..fields
            })
        }),
        Field::Day1 => parse_number(rest, input, pos, fields, 1, 2, |fields, value| {
            (1..=31).contains(&value).then_some(Fields {
                day: Some(value as u8),
                ..fields
            })
        }),
        Field::DayOrdinal => {
            for width in (1..=2).rev() {
                let Some(value) = digits_at(input, pos, width) else {
                    continue;
                };
                if !(1..=31).contains(&value) {
                    continue;
                }
                let day = value as u8;
                let suffix = ordinal_suffix(day);
                if match_name(input, pos + width, suffix).is_none() {
                    continue;
                }
                let next = Fields {
                    day: Some(day),
                    ..fields
                };
                if let Some(result) = parse_items(rest, input, pos + width + suffix.len(), next) {
                    return Some(result);
                }
            }
            None
        }
        Field::WeekdayFull | Field::WeekdayShort => {
            let (_, len) = match_weekday(input, pos)?;
            parse_items(rest, input, pos + len, fields)
        }
    }
}

fn parse_number(
    rest: &[PatternItem],
    input: &[char],
    pos: usize,
    fields: Fields,
    min: usize,
    max: usize,
    assign: fn(Fields, u32) -> Option<Fields>,
) -> Option<Fields> {
    for width in (min..=max).rev() {
        let Some(value) = digits_at(input, pos, width) else {
            continue;
        };
        let Some(next) = assign(fields, value) else {
            continue;
        };
        if let Some(result) = parse_items(rest, input, pos + width, next) {
            return Some(result);
        }
    }
    None
}

fn digits_at(input: &[char], pos: usize, width: usize) -> Option<u32> {
    let mut value = 0u32;
    for offset in 0..width {
        let digit = input.get(pos + offset)?.to_digit(10)?;
        value = value * 10 + digit;
    }
    Some(value)
}

fn match_name(input: &[char], pos: usize, name: &str) -> Option<usize> {
    let chars: Vec<char> = name.chars().collect();
    if pos + chars.len() > input.len() {
        return None;
    }
    input[pos..pos + chars.len()]
        .iter()
        .zip(chars.iter())
        .all(|(a, b)| a.eq_ignore_ascii_case(b))
        .then_some(chars.len())
}

fn match_month(input: &[char], pos: usize) -> Option<(u8, usize)> {
    for names in [MONTH_FULL, MONTH_SHORT] {
        for (index, name) in names.iter().enumerate() {
            if let Some(len) = match_name(input, pos, name) {
                return Some((index as u8 + 1, len));
            }
        }
    }
    None
}

fn match_weekday(input: &[char], pos: usize) -> Option<(Weekday, usize)> {
    for names in [WEEKDAY_FULL, WEEKDAY_SHORT] {
        for (index, name) in names.iter().enumerate() {
            if let Some(len) = match_name(input, pos, name) {
                return Some((Weekday::from_iso(index as u8 + 1)?, len));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn date(year: i32, month: u8, day: u8) -> JournalDate {
        JournalDate::new(year, month, day).expect("test date is valid")
    }

    fn fmt(date: JournalDate, pattern: &str) -> String {
        let format = JournalFormat {
            file_name: pattern.into(),
            page_title: pattern.into(),
        };
        format.format_title(date)
    }

    fn parse(date_text: &str, pattern: &str) -> Option<JournalDate> {
        let format = JournalFormat {
            file_name: pattern.into(),
            page_title: pattern.into(),
        };
        format.parse_title(date_text)
    }

    #[test]
    fn new_validates_calendar_dates() {
        assert_eq!(
            date(2026, 10, 31),
            JournalDate {
                year: 2026,
                month: 10,
                day: 31
            }
        );
        assert_eq!(JournalDate::new(2024, 2, 29), Some(date(2024, 2, 29)));
        assert_eq!(JournalDate::new(2000, 2, 29), Some(date(2000, 2, 29)));
        assert_eq!(JournalDate::new(1900, 2, 29), None);
        assert_eq!(JournalDate::new(2023, 2, 29), None);
        assert_eq!(JournalDate::new(2024, 4, 31), None);
        assert_eq!(JournalDate::new(2024, 0, 1), None);
        assert_eq!(JournalDate::new(2024, 13, 1), None);
        assert_eq!(JournalDate::new(2024, 1, 0), None);
        assert_eq!(JournalDate::new(0, 1, 1), None);
        assert_eq!(JournalDate::new(10_000, 1, 1), None);
    }

    #[test]
    fn weekday_matches_known_dates() {
        assert_eq!(date(1970, 1, 1).weekday(), Weekday::Thursday);
        assert_eq!(date(2000, 1, 1).weekday(), Weekday::Saturday);
        assert_eq!(date(2024, 2, 29).weekday(), Weekday::Thursday);
        assert_eq!(date(2026, 10, 31).weekday(), Weekday::Saturday);
        assert_eq!(date(1, 1, 1).weekday(), Weekday::Monday);
        assert_eq!(Weekday::Thursday.iso_number(), 4);
    }

    #[test]
    fn add_days_crosses_month_and_year_boundaries() {
        assert_eq!(date(2024, 2, 28).succ(), Some(date(2024, 2, 29)));
        assert_eq!(date(2024, 2, 29).succ(), Some(date(2024, 3, 1)));
        assert_eq!(date(2024, 12, 31).succ(), Some(date(2025, 1, 1)));
        assert_eq!(date(2024, 1, 1).pred(), Some(date(2023, 12, 31)));
        assert_eq!(date(1900, 2, 28).succ(), Some(date(1900, 3, 1)));
        assert_eq!(date(2024, 1, 1).add_days(366), Some(date(2025, 1, 1)));
        assert_eq!(date(2024, 1, 1).add_days(-366), Some(date(2022, 12, 31)));
        assert_eq!(date(9999, 12, 31).succ(), None);
        assert_eq!(date(1, 1, 1).pred(), None);
    }

    #[test]
    fn today_from_unix_uses_offset() {
        assert_eq!(today_from_unix(0, 0), Some(date(1970, 1, 1)));
        assert_eq!(today_from_unix(0, 8 * 3600), Some(date(1970, 1, 1)));
        assert_eq!(today_from_unix(0, -12 * 3600), Some(date(1969, 12, 31)));
        assert_eq!(today_from_unix(86_400, 0), Some(date(1970, 1, 2)));
        assert_eq!(today_from_unix(1_700_000_000, 0), Some(date(2023, 11, 14)));
        assert_eq!(
            today_from_unix(1_700_000_000, 7200),
            Some(date(2023, 11, 15))
        );
        assert_eq!(
            today_from_unix(1_700_000_000, -3600),
            Some(date(2023, 11, 14))
        );
    }

    #[test]
    fn default_formats() {
        let format = JournalFormat::default();
        let day = date(2026, 10, 31);
        assert_eq!(format.format_file_stem(day), "2026_10_31");
        assert_eq!(format.format_title(day), "Oct 31st, 2026");
        assert_eq!(format.parse_file_stem("2026_10_31"), Some(day));
        assert_eq!(format.parse_title("Oct 31st, 2026"), Some(day));
    }

    #[test]
    fn formats_every_supported_token() {
        let day = date(2026, 10, 1);
        assert_eq!(fmt(day, "yyyy|yy"), "2026|26");
        assert_eq!(fmt(day, "MMMM|MMM|MM|M"), "October|Oct|10|10");
        assert_eq!(fmt(date(2026, 1, 5), "MMMM|MMM|MM|M"), "January|Jan|01|1");
        assert_eq!(fmt(date(2026, 10, 5), "dd|d|do"), "05|5|5th");
        assert_eq!(fmt(day, "EEEE|EEE|E|EE"), "Thursday|Thu|Thu|Thu");
    }

    #[test]
    fn formats_ordinal_days() {
        let expected = [
            "1st", "2nd", "3rd", "4th", "5th", "6th", "7th", "8th", "9th", "10th", "11th", "12th",
            "13th", "14th", "15th", "16th", "17th", "18th", "19th", "20th", "21st", "22nd", "23rd",
            "24th", "25th", "26th", "27th", "28th", "29th", "30th", "31st",
        ];
        for (index, want) in expected.iter().enumerate() {
            let day = date(2026, 1, index as u8 + 1);
            assert_eq!(&fmt(day, "do"), want);
        }
    }

    #[test]
    fn parses_ordinal_days() {
        for day in 1..=31u8 {
            let text = fmt(date(2026, 1, day), "yyyy-MM-do");
            assert_eq!(
                parse(&text, "yyyy-MM-do"),
                Some(date(2026, 1, day)),
                "{text}"
            );
        }
        assert_eq!(parse("2026-01-32nd", "yyyy-MM-do"), None);
        assert_eq!(parse("2026-01-1th", "yyyy-MM-do"), None);
        assert_eq!(parse("2026-01-1stx", "yyyy-MM-do"), None);
    }

    #[test]
    fn parses_quoted_literals() {
        let format = JournalFormat {
            file_name: "'Journal 'yyyy-MM-dd".into(),
            page_title: "'Journal 'yyyy-MM-dd".into(),
        };
        assert_eq!(
            format.format_title(date(2026, 10, 31)),
            "Journal 2026-10-31"
        );
        assert_eq!(
            format.parse_title("Journal 2026-10-31"),
            Some(date(2026, 10, 31))
        );
        assert_eq!(format.parse_title("Journal 2026-10-32"), None);
        assert_eq!(format.parse_title("journal 2026-10-31"), None);

        assert_eq!(fmt(date(2026, 10, 31), "''yyyy-MM-dd"), "'2026-10-31");
        assert_eq!(
            parse("'2026-10-31", "''yyyy-MM-dd"),
            Some(date(2026, 10, 31))
        );
        assert_eq!(
            parse("第 2026 年 10 月 31 日", "'第' yyyy '年' M '月' d '日'"),
            Some(date(2026, 10, 31))
        );
    }

    #[test]
    fn parses_custom_patterns() {
        assert_eq!(parse("26-1-5", "yy-M-d"), Some(date(2026, 1, 5)));
        assert_eq!(parse("20260105", "yyyyMMdd"), Some(date(2026, 1, 5)));
        assert_eq!(parse("2026-10-31", "yyyy-MM-dd"), Some(date(2026, 10, 31)));
        assert_eq!(parse("2026-10-31", "yyyy_MM_dd"), None);
        assert_eq!(parse("2026_02_30", "yyyy_MM_dd"), None);
        assert_eq!(parse("2026_13_01", "yyyy_MM_dd"), None);
        assert_eq!(parse("", "yyyy_MM_dd"), None);
        assert_eq!(parse("2026_10_31 ", "yyyy_MM_dd"), None);
        assert_eq!(parse("2026_10_31x", "yyyy_MM_dd"), None);
    }

    #[test]
    fn parses_month_names_case_insensitively() {
        assert_eq!(
            parse("october 31, 2026", "MMMM d, yyyy"),
            Some(date(2026, 10, 31))
        );
        assert_eq!(
            parse("OCT 31, 2026", "MMM d, yyyy"),
            Some(date(2026, 10, 31))
        );
        assert_eq!(parse("Foo 31, 2026", "MMM d, yyyy"), None);
    }

    #[test]
    fn weekday_tokens_are_not_validated() {
        // 2026-10-31 is a Saturday; the parser trusts the date fields only.
        let parsed = parse("Friday, Oct 31st, 2026", "EEEE, MMM do, yyyy");
        assert_eq!(parsed, Some(date(2026, 10, 31)));
    }

    proptest! {
        #[test]
        fn default_formats_roundtrip(
            year in 1i32..=9999,
            month in 1u8..=12,
            day in 1u8..=28,
        ) {
            let date = JournalDate::new(year, month, day).expect("generated date is valid");
            let format = JournalFormat::default();
            prop_assert_eq!(
                format.parse_file_stem(&format.format_file_stem(date)),
                Some(date)
            );
            prop_assert_eq!(
                format.parse_title(&format.format_title(date)),
                Some(date)
            );
        }
    }
}
