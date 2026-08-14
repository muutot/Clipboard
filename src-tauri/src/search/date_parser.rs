use chrono::{DateTime, Datelike, Duration, Local};

/// Relative-date periods understood from a free-text search query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DatePeriod {
    Today,
    Yesterday,
    ThisWeek,
    ThisMonth,
}

/// The supported Chinese phrases mapped to their [`DatePeriod`].
///
/// Longer phrases are listed first so an exact match is preferred; aliases such
/// as `今日` / `昨日` / `这周` / `这个月` are accepted alongside the canonical form.
const RULES: &[(&str, DatePeriod)] = &[
    ("今天", DatePeriod::Today),
    ("今日", DatePeriod::Today),
    ("昨天", DatePeriod::Yesterday),
    ("昨日", DatePeriod::Yesterday),
    ("本周", DatePeriod::ThisWeek),
    ("这周", DatePeriod::ThisWeek),
    ("本月", DatePeriod::ThisMonth),
    ("这个月", DatePeriod::ThisMonth),
];

/// Extracts a Chinese relative-date phrase from `input`.
///
/// Returns the matching `[start_ms, end_ms)` epoch-millisecond range expressed
/// in the user's local timezone, plus the remaining query text with every
/// matched phrase removed and whitespace collapsed. When no phrase is present
/// the range is `None` and the original text is returned unchanged.
pub fn extract_date_range(input: &str) -> (Option<(i64, i64)>, String) {
    let mut remaining = input.to_owned();
    let mut range: Option<(i64, i64)> = None;

    // Apply every matching rule so multiple phrases are all stripped; the last
    // one wins for the returned range (an unusual input such as `本周 本月`).
    for (phrase, period) in RULES {
        if remaining.contains(phrase) {
            range = Some(period_range(*period));
            remaining = remaining.replace(phrase, " ");
        }
    }

    let remaining = remaining.split_whitespace().collect::<Vec<_>>().join(" ");
    (range, remaining)
}

/// Local midnight (00:00:00.000) of `dt` as epoch milliseconds.
fn local_midnight(dt: DateTime<Local>) -> i64 {
    dt.date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("midnight is always a valid time")
        .and_local_timezone(Local)
        .unwrap()
        .timestamp_millis()
}

/// `[start_ms, end_ms)` epoch-millisecond range for `period` in local time.
fn period_range(period: DatePeriod) -> (i64, i64) {
    let now = Local::now();
    match period {
        DatePeriod::Today => {
            let start = local_midnight(now);
            let end = local_midnight(now + Duration::days(1));
            (start, end)
        }
        DatePeriod::Yesterday => {
            let start = local_midnight(now - Duration::days(1));
            let end = local_midnight(now);
            (start, end)
        }
        DatePeriod::ThisWeek => {
            // Weeks start on Monday (ISO 8601), matching Chinese convention.
            let days_from_monday = now.weekday().num_days_from_monday() as i64;
            let monday = now - Duration::days(days_from_monday);
            let start = local_midnight(monday);
            let end = local_midnight(monday + Duration::days(7));
            (start, end)
        }
        DatePeriod::ThisMonth => {
            let start = local_midnight(now.with_day(1).unwrap_or(now));
            let next_month = if now.month() == 12 {
                now.with_year(now.year() + 1)
                    .and_then(|d| d.with_month(1))
                    .unwrap_or(now)
            } else {
                now.with_month(now.month() + 1).unwrap_or(now)
            };
            let end = local_midnight(next_month);
            (start, end)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn no_phrase_keeps_original_text_and_no_range() {
        let (range, rest) = extract_date_range("发票");
        assert!(range.is_none());
        assert_eq!(rest, "发票");
    }

    #[test]
    fn today_strips_phrase_and_keeps_content() {
        let (range, rest) = extract_date_range("今天发票");
        let (start, end) = range.expect("today should produce a range");
        assert!(start < end);
        assert_eq!(rest, "发票");
    }

    #[test]
    fn spaced_query_strips_phrase() {
        let (range, rest) = extract_date_range("昨天 发票 内容");
        assert!(range.is_some());
        assert_eq!(rest, "发票 内容");
    }

    #[test]
    fn yesterday_range_ends_at_today_midnight() {
        let (range, _) = extract_date_range("昨天");
        let (start, end) = range.unwrap();
        let (today_start, _) = period_range(DatePeriod::Today);
        assert_eq!(end, today_start);
        assert!(start < end);
    }

    #[test]
    fn this_week_starts_on_monday() {
        let (range, _) = extract_date_range("本周");
        let (start, _) = range.unwrap();
        let dt = Local.timestamp_millis_opt(start).single().unwrap();
        assert_eq!(dt.weekday().num_days_from_monday(), 0);
    }

    #[test]
    fn this_month_starts_on_first_day() {
        let (range, _) = extract_date_range("本月");
        let (start, _) = range.unwrap();
        let dt = Local.timestamp_millis_opt(start).single().unwrap();
        assert_eq!(dt.day(), 1);
    }

    #[test]
    fn aliases_are_recognized() {
        for phrase in ["今日", "昨日", "这周", "这个月"] {
            assert!(
                extract_date_range(phrase).0.is_some(),
                "{phrase} should be recognized"
            );
        }
    }

    #[test]
    fn multiple_phrases_all_stripped_last_wins() {
        let (range, rest) = extract_date_range("本周 发票 本月");
        assert!(range.is_some());
        assert_eq!(rest, "发票");
    }
}
