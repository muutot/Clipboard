use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    pub currency_values: Vec<String>,
    pub ip_addresses: Vec<String>,
    pub urls: Vec<String>,
}

impl Default for ContentMarkers {
    fn default() -> Self {
        Self {
            is_link: false,
            has_email: false,
            has_phone: false,
            has_color: false,
            has_date: false,
            has_currency: false,
            has_ip_address: false,
            has_url: false,
            emails: Vec::new(),
            phone_numbers: Vec::new(),
            color_values: Vec::new(),
            currency_values: Vec::new(),
            ip_addresses: Vec::new(),
            urls: Vec::new(),
        }
    }
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

    markers.is_link = is_standalone_link(trimmed);
    markers.has_date = contains_date(trimmed);

    markers
}

fn try_match(regex_str: &str, _text: &str) -> Option<regex_lite::Regex> {
    regex_lite::Regex::new(regex_str).ok()
}

fn extract_urls(text: &str) -> Vec<String> {
    let re = try_match(r"https?://[^\s<>{}|\^`\[\]]+", text);
    re.map_or(Vec::new(), |re| {
        re.find_iter(text)
            .map(|m| m.as_str().trim_end_matches('.').to_string())
            .collect()
    })
}

fn extract_emails(text: &str) -> Vec<String> {
    let re = try_match(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}", text);
    re.map_or(Vec::new(), |re| {
        re.find_iter(text).map(|m| m.as_str().to_string()).collect()
    })
}

fn extract_phone_numbers(text: &str) -> Vec<String> {
    let mut results = Vec::new();

    // Chinese mobile: 1[3-9]xxxxxxxxx (11 digits)
    if let Some(re) = try_match(r"\b1[3-9]\d{9}\b", text) {
        results.extend(re.find_iter(text).map(|m| m.as_str().to_string()));
    }

    // Chinese landline: 0xx-xxxxxxxx or 0xxx-xxxxxxx
    if let Some(re) = try_match(r"\b0\d{2}[\-]?\d{7,8}\b|\b0\d{3}[\-]?\d{7,8}\b", text) {
        results.extend(
            re.find_iter(text).map(|m| m.as_str().to_string())
        );
    }

    // International format: +xx xxx xxx xxx or +x (xxx) xxx-xxxx
    if let Some(re) = try_match(r"\+\d{1,3}[\s\-]?(?:\d{1,4}[\s\-]?){2,4}\d{2,4}", text) {
        results.extend(
            re.find_iter(text)
                .filter(|m| {
                    let digit_count = m.as_str().chars().filter(|c| c.is_ascii_digit()).count();
                    digit_count >= 8 && digit_count <= 15
                })
                .map(|m| m.as_str().to_string()),
        );
    }

    results
}

fn extract_colors(text: &str) -> Vec<String> {
    let mut results = Vec::new();

    if let Some(re) = try_match(r"#[0-9a-fA-F]{3,8}\b", text) {
        results.extend(
            re.find_iter(text)
                .filter(|m| {
                    let len = m.as_str().len();
                    len == 4 || len == 7 || len == 9
                })
                .map(|m| m.as_str().to_string()),
        );
    }

    if let Some(re) = try_match(
        r"rgba?\([^)]+\)",
        text,
    ) {
        results.extend(
            re.find_iter(text)
                .filter(|m| {
                    let s = m.as_str();
                    let digit_count = s.chars().filter(|c| c.is_ascii_digit()).count();
                    digit_count >= 3
                })
                .map(|m| m.as_str().to_string()),
        );
    }

    if let Some(re) = try_match(
        r"hsla?\([^)]+\)",
        text,
    ) {
        results.extend(
            re.find_iter(text)
                .filter(|m| m.as_str().contains('%'))
                .map(|m| m.as_str().to_string()),
        );
    }

    results
}

fn extract_currency(text: &str) -> Vec<String> {
    let mut results = Vec::new();

    let symbols = [
        "¥", "$", "€", "£", "USD", "CNY", "EUR", "GBP", "JPY",
    ];
    for symbol in symbols.iter() {
        if let Some(re) = try_match(
            &format!(
                r"{}\s*\d{{1,3}}(?:[,.]\d{{3}})*(?:\.\d{{2}})?",
                regex_lite::escape(symbol)
            ),
            text,
        ) {
            results.extend(re.find_iter(text).map(|m| m.as_str().to_string()));
        }
    }

    let suffix_labels = ["元", "美元", "欧元", "英镑", "日元"];
    for label in suffix_labels.iter() {
        if let Some(re) = try_match(
            &format!(r"\b\d{{1,3}}(?:[,.]\d{{3}})*(?:\.\d{{2}})?\s*{}", label),
            text,
        ) {
            results.extend(re.find_iter(text).map(|m| m.as_str().to_string()));
        }
    }

    if let Some(re) = try_match(r"\b\d{1,3}(?:[,.]\d{3})*(?:\.\d{2})?\s*円", text) {
        results.extend(re.find_iter(text).map(|m| m.as_str().to_string()));
    }

    results
}

fn extract_ip_addresses(text: &str) -> Vec<String> {
    let mut results = Vec::new();

    if let Some(re) = try_match(
        r"\b(?:(?:25[0-5]|2[0-4]\d|[01]?\d\d?)\.){3}(?:25[0-5]|2[0-4]\d|[01]?\d\d?)\b",
        text,
    ) {
        results.extend(re.find_iter(text).map(|m| m.as_str().to_string()));
    }

    if let Some(re) = try_match(
        r"\b(?:[0-9a-fA-F]{1,4}:){7}[0-9a-fA-F]{1,4}\b",
        text,
    ) {
        results.extend(re.find_iter(text).map(|m| m.as_str().to_string()));
    }

    results
}

fn is_standalone_link(text: &str) -> bool {
    if text.starts_with("http://") || text.starts_with("https://") {
        return !text.contains(char::is_whitespace);
    }
    false
}

fn contains_date(text: &str) -> bool {
    let patterns = [
        r"\b\d{4}[-/]\d{1,2}[-/]\d{1,2}\b",
        r"\b\d{1,2}[-/]\d{1,2}[-/]\d{4}\b",
        r"\b\d{4}年\d{1,2}月\d{1,2}日\b",
    ];
    for pattern in &patterns {
        if let Some(re) = try_match(pattern, text) {
            if re.is_match(text) {
                return true;
            }
        }
    }
    false
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
