use crate::content::ContentMarkers;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickAction {
    pub label: String,
    pub action_type: String,
    pub payload: String,
}

pub fn detect_actions(markers: &ContentMarkers) -> Vec<QuickAction> {
    let mut actions = Vec::new();

    for email in &markers.emails {
        actions.push(QuickAction {
            label: format!("Send email to {}", email),
            action_type: "open".to_owned(),
            payload: format!("mailto:{}", email),
        });
    }

    for phone in &markers.phone_numbers {
        let cleaned = phone
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == '+')
            .collect::<String>();
        actions.push(QuickAction {
            label: format!("Call {}", phone),
            action_type: "open".to_owned(),
            payload: format!("tel:{}", cleaned),
        });
    }

    for url in &markers.urls {
        actions.push(QuickAction {
            label: format!("Open {}", truncate(url, 40)),
            action_type: "open".to_owned(),
            payload: url.clone(),
        });
    }

    for date in &markers.date_values {
        actions.push(QuickAction {
            label: format!("View date {}", date),
            action_type: "viewDate".to_owned(),
            payload: date.clone(),
        });
    }

    for color in &markers.color_values {
        actions.push(QuickAction {
            label: format!("Copy color {}", color),
            action_type: "copy".to_owned(),
            payload: color.clone(),
        });
    }

    for currency in &markers.currency_values {
        actions.push(QuickAction {
            label: format!("Copy amount {}", currency),
            action_type: "copy".to_owned(),
            payload: currency.clone(),
        });
    }

    for ip in &markers.ip_addresses {
        actions.push(QuickAction {
            label: format!("Copy IP {}", ip),
            action_type: "copy".to_owned(),
            payload: ip.clone(),
        });
    }

    actions
}

fn truncate(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_owned()
    } else {
        let mut boundary = max_len;
        while boundary > 0 && !text.is_char_boundary(boundary) {
            boundary -= 1;
        }
        format!("{}...", &text[..boundary])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::detect_markers;

    #[test]
    fn detects_email_action() {
        let markers = detect_markers("contact user@example.com");
        let actions = detect_actions(&markers);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].action_type, "open");
        assert_eq!(actions[0].payload, "mailto:user@example.com");
    }

    #[test]
    fn detects_phone_action() {
        let markers = detect_markers("call 138-1234-5678");
        let actions = detect_actions(&markers);
        assert!(!actions.is_empty());
        assert!(actions
            .iter()
            .any(|a| a.action_type == "open" && a.payload.starts_with("tel:")));
    }

    #[test]
    fn detects_url_action() {
        let markers = detect_markers("visit https://example.com/page");
        let actions = detect_actions(&markers);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].payload, "https://example.com/page");
    }

    #[test]
    fn detects_color_action() {
        let markers = detect_markers("background: #ff4655");
        let actions = detect_actions(&markers);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].action_type, "copy");
        assert_eq!(actions[0].payload, "#ff4655");
    }

    #[test]
    fn detects_date_action_with_normalized_payload() {
        let markers = detect_markers("meeting on 2024/1/15");
        let actions = detect_actions(&markers);

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].label, "View date 2024-01-15");
        assert_eq!(actions[0].action_type, "viewDate");
        assert_eq!(actions[0].payload, "2024-01-15");
    }

    #[test]
    fn date_actions_stay_before_copy_actions() {
        let markers = detect_markers("user@example.com 2024-01-15 #ff4655");
        let actions = detect_actions(&markers);

        assert_eq!(
            actions
                .iter()
                .map(|action| action.action_type.as_str())
                .collect::<Vec<_>>(),
            vec!["open", "viewDate", "copy"]
        );
    }

    #[test]
    fn ambiguous_date_does_not_create_an_action() {
        let markers = detect_markers("deadline: 03/04/2024");
        let actions = detect_actions(&markers);

        assert!(markers.has_date);
        assert!(actions.is_empty());
    }

    #[test]
    fn detects_currency_action() {
        let markers = detect_markers("price: $1,234.56");
        let actions = detect_actions(&markers);
        assert!(!actions.is_empty());
        assert_eq!(actions[0].action_type, "copy");
    }

    #[test]
    fn detects_ip_action() {
        let markers = detect_markers("server at 192.168.1.1");
        let actions = detect_actions(&markers);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].action_type, "copy");
        assert_eq!(actions[0].payload, "192.168.1.1");
    }

    #[test]
    fn plain_text_returns_no_actions() {
        let markers = detect_markers("hello world");
        let actions = detect_actions(&markers);
        assert!(actions.is_empty());
    }

    #[test]
    fn truncate_does_not_split_multi_byte_characters() {
        assert_eq!(truncate("中文中文中文", 4), "中...");
        assert_eq!(truncate("中文中文中文", 6), "中文...");
    }

    #[test]
    fn truncate_keeps_short_text_unchanged() {
        let text = "short";
        assert_eq!(truncate(text, 5), "short");
    }
}
