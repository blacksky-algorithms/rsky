//! Small presentation helpers the account pages share: relative dates,
//! calendar dates, and a device name read from a user agent.

/// "just now", "3 minutes ago", "yesterday", "12 days ago".
pub fn date_ago(now: u64, then: u64) -> String {
    let seconds = now.saturating_sub(then);
    if seconds < 60 {
        return "just now".to_string();
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return plural(minutes, "minute");
    }
    let hours = minutes / 60;
    if hours < 24 {
        return plural(hours, "hour");
    }
    let days = hours / 24;
    if days == 1 {
        return "yesterday".to_string();
    }
    format!("{days} days ago")
}

fn plural(count: u64, unit: &str) -> String {
    if count == 1 {
        format!("1 {unit} ago")
    } else {
        format!("{count} {unit}s ago")
    }
}

/// "Sep 17, 2026".
pub fn calendar_date(secs: u64) -> String {
    chrono::DateTime::from_timestamp(i64::try_from(secs).unwrap_or(i64::MAX), 0)
        .map(|date| date.format("%b %-d, %Y").to_string())
        .unwrap_or_default()
}

/// The device as the reference names it: the operating system, plus the
/// browser on anything that is not a phone.
pub fn browser_name(user_agent: Option<&str>) -> Option<String> {
    let ua = user_agent?.trim();
    if ua.is_empty() {
        return None;
    }
    let mobile = ua.contains("iPhone") || ua.contains("iPod") || ua.contains("Android");
    let os = if ua.contains("iPhone") || ua.contains("iPad") || ua.contains("iPod") {
        Some("iOS")
    } else if ua.contains("Android") {
        Some("Android")
    } else if ua.contains("Windows") {
        Some("Windows")
    } else if ua.contains("CrOS") {
        Some("Chrome OS")
    } else if ua.contains("Mac OS X") || ua.contains("Macintosh") {
        Some("macOS")
    } else if ua.contains("Linux") {
        Some("Linux")
    } else {
        None
    };
    let browser = if ua.contains("Edg/") || ua.contains("EdgiOS/") {
        Some("Edge")
    } else if ua.contains("OPR/") || ua.contains("Opera") {
        Some("Opera")
    } else if ua.contains("Firefox/") || ua.contains("FxiOS/") {
        Some("Firefox")
    } else if ua.contains("Chrome/") || ua.contains("CriOS/") {
        Some("Chrome")
    } else if ua.contains("Safari/") {
        Some("Safari")
    } else {
        None
    };
    let parts: Vec<&str> = if mobile {
        os.into_iter().collect()
    } else {
        os.into_iter().chain(browser).collect()
    };
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" \u{2022} "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_dates_follow_the_reference_buckets() {
        assert_eq!(date_ago(1000, 1000), "just now");
        assert_eq!(date_ago(1000, 941), "just now");
        assert_eq!(date_ago(1000, 940), "1 minute ago");
        assert_eq!(date_ago(1000, 700), "5 minutes ago");
        assert_eq!(date_ago(10_000, 10_000 - 3600), "1 hour ago");
        assert_eq!(date_ago(100_000, 100_000 - 7200), "2 hours ago");
        assert_eq!(date_ago(1_000_000, 1_000_000 - 86_400), "yesterday");
        assert_eq!(date_ago(1_000_000, 1_000_000 - 3 * 86_400), "3 days ago");
        assert_eq!(date_ago(5, 50), "just now");
    }

    #[test]
    fn calendar_dates_are_short() {
        assert_eq!(calendar_date(1_789_000_000), "Sep 10, 2026");
        assert_eq!(calendar_date(u64::MAX), "");
    }

    #[test]
    fn device_names_come_from_the_user_agent() {
        assert_eq!(browser_name(None), None);
        assert_eq!(browser_name(Some("  ")), None);
        assert_eq!(browser_name(Some("curl/8.0")), None);
        assert_eq!(
            browser_name(Some(
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15"
            ))
            .as_deref(),
            Some("macOS \u{2022} Safari")
        );
        assert_eq!(
            browser_name(Some(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36 Edg/120.0"
            ))
            .as_deref(),
            Some("Windows \u{2022} Edge")
        );
        assert_eq!(
            browser_name(Some(
                "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36"
            ))
            .as_deref(),
            Some("Linux \u{2022} Chrome")
        );
        assert_eq!(
            browser_name(Some("Mozilla/5.0 (X11; CrOS x86_64) Firefox/121.0")).as_deref(),
            Some("Chrome OS \u{2022} Firefox")
        );
        assert_eq!(
            browser_name(Some("Mozilla/5.0 (Linux; Android 14) Chrome/120.0 Mobile")).as_deref(),
            Some("Android")
        );
        assert_eq!(
            browser_name(Some(
                "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) CriOS/120"
            ))
            .as_deref(),
            Some("iOS")
        );
        assert_eq!(
            browser_name(Some(
                "Mozilla/5.0 (iPad; CPU OS 17_0 like Mac OS X) Safari/605.1"
            ))
            .as_deref(),
            Some("iOS \u{2022} Safari")
        );
        assert_eq!(
            browser_name(Some("Opera/9.80 (Windows NT 6.1) OPR/100")).as_deref(),
            Some("Windows \u{2022} Opera")
        );
        assert_eq!(
            browser_name(Some("Firefox/121.0")).as_deref(),
            Some("Firefox")
        );
    }
}
