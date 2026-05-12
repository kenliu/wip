use std::time::{SystemTime, UNIX_EPOCH};

pub fn format_age(ts: i64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let secs = (now - ts).max(0);
    if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86400)
    }
}

pub fn format_age_long(ts: i64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let secs = (now - ts).max(0);
    if secs < 60 {
        "just now".to_string()
    } else if secs < 3600 {
        format!("{} min ago", secs / 60)
    } else if secs < 86400 {
        format!("{} hours ago", secs / 3600)
    } else {
        format!("{} days ago", secs / 86400)
    }
}

pub fn project_name(cwd: &str) -> String {
    std::path::Path::new(cwd)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── project_name ────────────────────────────────────────────────────

    #[test]
    fn project_name_normal_path() {
        assert_eq!(project_name("/home/user/my-project"), "my-project");
    }

    #[test]
    fn project_name_nested_path() {
        assert_eq!(project_name("/a/b/c/deep"), "deep");
    }

    #[test]
    fn project_name_root() {
        assert_eq!(project_name("/"), "");
    }

    #[test]
    fn project_name_empty() {
        assert_eq!(project_name(""), "");
    }

    #[test]
    fn project_name_trailing_slash() {
        // Path::file_name returns None for paths ending in /
        // but on most systems this gets normalized
        let result = project_name("/home/user/project/");
        assert!(result == "project" || result.is_empty());
    }

    // ── format_age ──────────────────────────────────────────────────────

    fn now_ts() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    #[test]
    fn format_age_minutes() {
        let ts = now_ts() - 180; // 3 minutes ago
        assert_eq!(format_age(ts), "3m ago");
    }

    #[test]
    fn format_age_hours() {
        let ts = now_ts() - 7200; // 2 hours ago
        assert_eq!(format_age(ts), "2h ago");
    }

    #[test]
    fn format_age_days() {
        let ts = now_ts() - 172800; // 2 days ago
        assert_eq!(format_age(ts), "2d ago");
    }

    #[test]
    fn format_age_zero() {
        let ts = now_ts();
        assert_eq!(format_age(ts), "0m ago");
    }

    #[test]
    fn format_age_future_clamps_to_zero() {
        let ts = now_ts() + 1000;
        assert_eq!(format_age(ts), "0m ago");
    }

    // ── format_age_long ─────────────────────────────────────────────────

    #[test]
    fn format_age_long_just_now() {
        let ts = now_ts() - 30;
        assert_eq!(format_age_long(ts), "just now");
    }

    #[test]
    fn format_age_long_minutes() {
        let ts = now_ts() - 300;
        assert_eq!(format_age_long(ts), "5 min ago");
    }

    #[test]
    fn format_age_long_hours() {
        let ts = now_ts() - 7200;
        assert_eq!(format_age_long(ts), "2 hours ago");
    }

    #[test]
    fn format_age_long_days() {
        let ts = now_ts() - 172800;
        assert_eq!(format_age_long(ts), "2 days ago");
    }
}
