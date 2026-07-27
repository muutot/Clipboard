use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentMarkers {
    pub is_link: bool,
    pub has_email: bool,
    pub has_phone: bool,
    pub has_color: bool,
    pub has_date: bool,
    pub has_currency: bool,
    pub has_ip_address: bool,
    pub has_url: bool,
    pub emails: Vec<String>,
    pub phone_numbers: Vec<String>,
    pub color_values: Vec<String>,
    #[serde(default)]
    pub date_values: Vec<String>,
    pub currency_values: Vec<String>,
    pub ip_addresses: Vec<String>,
    pub urls: Vec<String>,
}

pub fn detect_markers(text: &str) -> ContentMarkers {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return ContentMarkers::default();
    }

    let mut markers = ContentMarkers::default();

    markers.urls = extract_urls(trimmed);
    markers.has_url = !markers.urls.is_empty();

    markers.emails = extract_emails(trimmed);
    markers.has_email = !markers.emails.is_empty();

    markers.phone_numbers = extract_phone_numbers(trimmed);
    markers.has_phone = !markers.phone_numbers.is_empty();

    markers.color_values = extract_colors(trimmed);
    markers.has_color = !markers.color_values.is_empty();

    markers.currency_values = extract_currency(trimmed);
    markers.has_currency = !markers.currency_values.is_empty();

    markers.ip_addresses = extract_ip_addresses(trimmed);
    markers.has_ip_address = !markers.ip_addresses.is_empty();

    let dates = extract_dates(trimmed);
    markers.has_date = dates.has_date;
    markers.date_values = dates.normalized_values;

    markers.is_link = is_standalone_link(trimmed);

    markers
}

use std::sync::LazyLock;

static RE_IPV4: LazyLock<regex_lite::Regex> = LazyLock::new(|| {
    regex_lite::Regex::new(r"\b(?:(?:25[0-5]|2[0-4]\d|[01]?\d\d?)\.){3}(?:25[0-5]|2[0-4]\d|[01]?\d\d?)\b").unwrap()
});
static RE_IPV6: LazyLock<regex_lite::Regex> = LazyLock::new(|| {
    regex_lite::Regex::new(r"\b(?:[0-9a-fA-F]{1,4}:){7}[0-9a-fA-F]{1,4}\b").unwrap()
});
static RE_DATE_NUMERIC: LazyLock<regex_lite::Regex> = LazyLock::new(|| {
    regex_lite::Regex::new(r"\b(\d{1,2})[-/](\d{1,2})[-/](\d{4})\b").unwrap()
});
static RE_DATE_ISO: LazyLock<regex_lite::Regex> = LazyLock::new(|| {
    regex_lite::Regex::new(r"\b\d{4}[-/]\d{1,2}[-/]\d{1,2}\b").unwrap()
});
static RE_DATE_CN: LazyLock<regex_lite::Regex> = LazyLock::new(|| {
    regex_lite::Regex::new(r"\b(\d{4})年(\d{1,2})月(\d{1,2})日").unwrap()
});

static RE_URL: LazyLock<regex_lite::Regex> =
    LazyLock::new(|| regex_lite::Regex::new(r"https?://[^\s<>{}|\^`\[\]]+").unwrap());
static RE_EMAIL: LazyLock<regex_lite::Regex> =
    LazyLock::new(|| regex_lite::Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").unwrap());
static RE_PHONE_CN: LazyLock<regex_lite::Regex> =
    LazyLock::new(|| regex_lite::Regex::new(r"\b1[3-9]\d(?:[- ]?\d{4}){2}\b").unwrap());
static RE_PHONE_LANDLINE: LazyLock<regex_lite::Regex> =
    LazyLock::new(|| regex_lite::Regex::new(r"\b0\d{2}[\-]?\d{7,8}\b|\b0\d{3}[\-]?\d{7,8}\b").unwrap());
static RE_PHONE_INTL: LazyLock<regex_lite::Regex> =
    LazyLock::new(|| regex_lite::Regex::new(r"\+\d{1,3}[\s\-]?(?:\d{1,4}[\s\-]?){2,4}\d{2,4}").unwrap());
static RE_COLOR_HEX: LazyLock<regex_lite::Regex> =
    LazyLock::new(|| regex_lite::Regex::new(r"#[0-9a-fA-F]{3,8}\b").unwrap());
static RE_COLOR_RGB: LazyLock<regex_lite::Regex> =
    LazyLock::new(|| regex_lite::Regex::new(r"rgba?\([^)]+\)").unwrap());
static RE_COLOR_HSL: LazyLock<regex_lite::Regex> =
    LazyLock::new(|| regex_lite::Regex::new(r"hsla?\([^)]+\)").unwrap());

fn extract_urls(text: &str) -> Vec<String> {
    RE_URL.find_iter(text).map(|m| m.as_str().trim_end_matches('.').to_string()).collect()
}

fn extract_emails(text: &str) -> Vec<String> {
    RE_EMAIL.find_iter(text).map(|m| m.as_str().to_string()).collect()
}

fn extract_phone_numbers(text: &str) -> Vec<String> {
    let mut results = Vec::new();
    results.extend(RE_PHONE_CN.find_iter(text).map(|m| m.as_str().to_string()));
    results.extend(RE_PHONE_LANDLINE.find_iter(text).map(|m| m.as_str().to_string()));
    results.extend(
        RE_PHONE_INTL.find_iter(text)
            .filter(|m| {
                let digit_count = m.as_str().chars().filter(|c| c.is_ascii_digit()).count();
                (8..=15).contains(&digit_count)
            })
            .map(|m| m.as_str().to_string()),
    );
    results
}

fn extract_colors(text: &str) -> Vec<String> {
    let mut results = Vec::new();
    results.extend(
        RE_COLOR_HEX.find_iter(text)
            .filter(|m| {
                let len = m.as_str().len();
                len == 4 || len == 7 || len == 9
            })
            .map(|m| m.as_str().to_string()),
    );
    results.extend(
        RE_COLOR_RGB.find_iter(text)
            .filter(|m| {
                let s = m.as_str();
                s.chars().filter(|c| c.is_ascii_digit()).count() >= 3
            })
            .map(|m| m.as_str().to_string()),
    );
    results.extend(
        RE_COLOR_HSL.find_iter(text)
            .filter(|m| m.as_str().contains('%'))
            .map(|m| m.as_str().to_string()),
    );
    results
}

static RE_CURRENCY: LazyLock<regex_lite::Regex> = LazyLock::new(|| {
    let symbols = ["¥", "$", "€", "£", "USD", "CNY", "EUR", "GBP", "JPY"];
    let suffix_labels = ["元", "美元", "欧元", "英镑", "日元", "円"];
    let mut pattern = String::from("(");
    // Prefix symbols: e.g. ¥100, $99.99
    for (i, s) in symbols.iter().enumerate() {
        if i > 0 { pattern.push('|'); }
        pattern.push_str(&regex_lite::escape(s));
        pattern.push_str(r"\s*\d{1,3}(?:[,.]\d{3})*(?:\.\d{2})?");
    }
    // Suffix labels: e.g. 100元, 99.99美元
    for label in suffix_labels.iter() {
        pattern.push('|');
        pattern.push_str(r"\d{1,3}(?:[,.]\d{3})*(?:\.\d{2})?\s*");
        pattern.push_str(label);
    }
    pattern.push(')');
    regex_lite::Regex::new(&pattern).unwrap()
});

fn extract_currency(text: &str) -> Vec<String> {
    RE_CURRENCY.find_iter(text).map(|m| m.as_str().to_string()).collect()
}

fn extract_ip_addresses(text: &str) -> Vec<String> {
    let mut results: Vec<String> = RE_IPV4.find_iter(text).map(|m| m.as_str().to_string()).collect();
    results.extend(RE_IPV6.find_iter(text).map(|m| m.as_str().to_string()));
    results
}

fn is_standalone_link(text: &str) -> bool {
    if text.starts_with("http://") || text.starts_with("https://") {
        return !text.contains(char::is_whitespace);
    }
    false
}

#[derive(Default)]
struct DateExtraction {
    has_date: bool,
    normalized_values: Vec<String>,
}

fn extract_dates(text: &str) -> DateExtraction {
    let mut dates = DateExtraction::default();

    for captures in RE_DATE_ISO.captures_iter(text).chain(RE_DATE_CN.captures_iter(text)) {
        let Some((year, month, day)) = parse_date_components(&captures) else { continue };
        dates.record_unambiguous(year, month, day);
    }

    for captures in RE_DATE_NUMERIC.captures_iter(text) {
        let Some((first, second, year)) = parse_date_components(&captures) else { continue };
        dates.record_trailing_year(year, first, second);
    }

    dates
}

fn parse_date_components(captures: &regex_lite::Captures<'_>) -> Option<(u32, u32, u32)> {
    Some((
        captures.get(1)?.as_str().parse().ok()?,
        captures.get(2)?.as_str().parse().ok()?,
        captures.get(3)?.as_str().parse().ok()?,
    ))
}

impl DateExtraction {
    fn record_unambiguous(&mut self, year: u32, month: u32, day: u32) {
        if !is_valid_date(year, month, day) {
            return;
        }

        self.has_date = true;
        let normalized = format!("{year:04}-{month:02}-{day:02}");
        if !self.normalized_values.contains(&normalized) {
            self.normalized_values.push(normalized);
        }
    }

    fn record_trailing_year(&mut self, year: u32, first: u32, second: u32) {
        let day_first = is_valid_date(year, second, first);
        let month_first = is_valid_date(year, first, second);

        match (day_first, month_first) {
            (true, false) => self.record_unambiguous(year, second, first),
            (false, true) => self.record_unambiguous(year, first, second),
            (true, true) if first == second => self.record_unambiguous(year, first, second),
            (true, true) => self.has_date = true,
            (false, false) => {}
        }
    }
}

fn is_valid_date(year: u32, month: u32, day: u32) -> bool {
    if year == 0 || day == 0 {
        return false;
    }

    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => return false,
    };
    day <= days_in_month
}

fn is_leap_year(year: u32) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_urls() {
        let markers = detect_markers("check https://example.com/page for details");
        assert!(markers.has_url);
        assert_eq!(markers.urls, vec!["https://example.com/page"]);
    }

    #[test]
    fn detects_standalone_link() {
        let markers = detect_markers("https://github.com/user/repo");
        assert!(markers.is_link);
    }

    #[test]
    fn detects_email() {
        let markers = detect_markers("contact user@example.com for help");
        assert!(markers.has_email);
        assert_eq!(markers.emails, vec!["user@example.com"]);
    }

    #[test]
    fn detects_phone() {
        let markers = detect_markers("call 13812345678 or +86 10 12345678");
        assert!(markers.has_phone);
        assert!(!markers.phone_numbers.is_empty());
    }

    #[test]
    fn detects_hex_color() {
        let markers = detect_markers("background: #ff4655 and #333");
        assert!(markers.has_color);
        assert!(markers.color_values.iter().any(|c| c == "#ff4655"));
    }

    #[test]
    fn detects_rgb_color() {
        let markers = detect_markers("color: rgb(255, 70, 85)");
        assert!(markers.has_color);
    }

    #[test]
    fn detects_currency() {
        let markers = detect_markers("price: $1,234.56");
        assert!(markers.has_currency);
    }

    #[test]
    fn detects_ipv4() {
        let markers = detect_markers("server at 192.168.1.1 is ready");
        assert!(markers.has_ip_address);
    }

    #[test]
    fn detects_ipv6() {
        let markers = detect_markers("listen on 2001:0db8:0000:0000:0000:0000:0000:0001");
        assert!(markers.has_ip_address);
    }

    #[test]
    fn detects_date() {
        let markers = detect_markers("meeting on 2024-01-15");
        assert!(markers.has_date);
        assert_eq!(markers.date_values, vec!["2024-01-15"]);
    }

    #[test]
    fn normalizes_supported_unambiguous_date_formats() {
        let markers = detect_markers("2024/1/5, 2024年01月05日, 31/01/2024, and 01/31/2024");

        assert!(markers.has_date);
        assert_eq!(markers.date_values, vec!["2024-01-05", "2024-01-31"]);
    }

    #[test]
    fn preserves_ambiguous_date_detection_without_guessing_a_value() {
        let markers = detect_markers("deadline: 03/04/2024");

        assert!(markers.has_date);
        assert!(markers.date_values.is_empty());
    }

    #[test]
    fn validates_month_lengths_and_leap_years() {
        let markers =
            detect_markers("valid 2024-02-29; invalid 2023-02-29, 2024-04-31, and 2024-13-01");

        assert!(markers.has_date);
        assert_eq!(markers.date_values, vec!["2024-02-29"]);
    }

    #[test]
    fn invalid_date_like_text_is_not_marked_as_a_date() {
        let markers = detect_markers("2023-02-29 31/31/2024");

        assert!(!markers.has_date);
        assert!(markers.date_values.is_empty());
    }

    #[test]
    fn empty_input_returns_default() {
        let markers = detect_markers("");
        assert_eq!(markers, ContentMarkers::default());
    }

    #[test]
    fn plain_text_no_markers() {
        let markers = detect_markers("hello world this is plain text");
        assert!(!markers.has_url);
        assert!(!markers.has_email);
        assert!(!markers.has_phone);
        assert!(!markers.has_color);
        assert!(!markers.has_currency);
        assert!(!markers.has_ip_address);
        assert!(!markers.is_link);
        assert!(!markers.has_date);
    }

    #[test]
    fn dates_are_not_phones() {
        let markers = detect_markers("2024-01-15");
        assert!(!markers.has_phone);
        assert!(markers.has_date);
    }

    #[test]
    fn ip_addresses_are_not_phones() {
        let markers = detect_markers("192.168.1.1");
        assert!(!markers.has_phone);
        assert!(markers.has_ip_address);
    }
}
