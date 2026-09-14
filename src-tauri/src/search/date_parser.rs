use chrono::{DateTime, Datelike, Days, Duration, Local, NaiveDate};

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

/// Local midnight (00:00:00.000) of `date` as epoch milliseconds.
///
/// On DST-transition days local midnight may be ambiguous (clock repeated) or
/// nonexistent (clock skipped). Both cases fall back to the current fixed
/// offset instead of panicking inside a search command; the resulting range
/// boundary can be off by at most one hour on those rare days.
fn local_midnight(date: NaiveDate) -> i64 {
    let naive = date
        .and_hms_opt(0, 0, 0)
        .expect("midnight is always a valid time");
    match naive.and_local_timezone(Local) {
        chrono::LocalResult::Single(midnight) => midnight.timestamp_millis(),
        chrono::LocalResult::Ambiguous(_, _) | chrono::LocalResult::None => {
            let offset = *Local::now().offset();
            chrono::DateTime::<Local>::from_naive_utc_and_offset(naive - offset, offset)
                .timestamp_millis()
        }
    }
}

/// `[start_ms, end_ms)` epoch-millisecond range for `period` in local time.
fn period_range(period: DatePeriod) -> (i64, i64) {
    period_range_at(period, Local::now())
}

/// Same as [`period_range`] but with an explicit "now", so the boundary math can
/// be unit-tested deterministically (month-end, leap years, DST).
fn period_range_at(period: DatePeriod, now: DateTime<Local>) -> (i64, i64) {
    let today = now.date_naive();
    match period {
        DatePeriod::Today => {
            // Calendar-day arithmetic, not `now + 24h`: on a DST fall-back day
            // the local day is 25 hours long, so an absolute 24h would end the
            // range before the day is over and could collapse it to empty.
            let end = local_midnight(today.succ_opt().unwrap_or(today));
            (local_midnight(today), end)
        }
        DatePeriod::Yesterday => {
            let start = local_midnight(today.pred_opt().unwrap_or(today));
            (start, local_midnight(today))
        }
        DatePeriod::ThisWeek => {
            // Weeks start on Monday (ISO 8601), matching Chinese convention.
            let days_from_monday = now.weekday().num_days_from_monday() as i64;
            let monday = (now - Duration::days(days_from_monday)).date_naive();
            let end = monday
                .checked_add_days(Days::new(7))
                .map(local_midnight)
                .unwrap_or_else(|| local_midnight(monday));
            (local_midnight(monday), end)
        }
        DatePeriod::ThisMonth => {
            let first = today.with_day(1).unwrap_or(today);
            let start = local_midnight(first);
            // Anchor on the 1st, then jump past the current month and normalize
            // back to day 1. Adding 31 days from the 1st always lands in the
            // next month (every month has at most 31 days), so the exclusive
            // end is the first instant of the following month regardless of the
            // current day-of-month.
            let first_of_next_month = first
                .checked_add_days(Days::new(31))
                .and_then(|date| date.with_day(1))
                .unwrap_or(first);
            (start, local_midnight(first_of_next_month))
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
    fn today_ends_at_tomorrow_midnight() {
        let now = Local::now();
        let (start, end) = period_range_at(DatePeriod::Today, now);
        assert!(start < end);
        let expected_end = local_midnight(now.date_naive().succ_opt().unwrap());
        assert_eq!(end, expected_end);
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

    fn local_dt(year: i32, month: u32, day: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(year, month, day, 12, 0, 0)
            .single()
            .expect("valid local date")
    }

    fn ymd(ms: i64) -> (i32, u32, u32) {
        let dt = Local.timestamp_millis_opt(ms).single().unwrap();
        (dt.year(), dt.month(), dt.day())
    }

    #[test]
    fn this_month_ends_on_the_first_of_next_month() {
        let (start, end) = period_range_at(DatePeriod::ThisMonth, local_dt(2026, 9, 14));
        assert_eq!(ymd(start), (2026, 9, 1));
        assert_eq!(ymd(end), (2026, 10, 1));
    }

    #[test]
    fn this_month_handles_a_31_day_month() {
        let (start, end) = period_range_at(DatePeriod::ThisMonth, local_dt(2026, 1, 31));
        assert_eq!(ymd(start), (2026, 1, 1));
        assert_eq!(ymd(end), (2026, 2, 1));
        assert!(end > start);
    }

    #[test]
    fn this_month_december_rolls_into_the_next_year() {
        let (_, end) = period_range_at(DatePeriod::ThisMonth, local_dt(2026, 12, 15));
        assert_eq!(ymd(end), (2027, 1, 1));
    }
}
