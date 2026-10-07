//! A stored agent session as listed in the Session view (port of `src/shared/sessions.ts`).

use serde::{Deserialize, Serialize};

/// A stored agent session of a project, as listed in the Session view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    /// The Agent SDK session ID, passed back to resume it.
    pub id: String,
    /// Display title: the SDK's custom title, summary or first prompt.
    pub title: String,
    /// Last modified time in milliseconds since epoch.
    pub last_modified: i64,
}

/// Local `YYYY-MM-DD HH:mm`, the timestamp shown in the session list.
/// Empty when the time cannot be converted.
pub fn format_session_time(ms: i64) -> String {
    let secs = ms.div_euclid(1000) as libc::time_t;
    // SAFETY: `tm` is plain old data, so the all-zero value is valid, and `localtime_r` only
    // writes into it.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let converted = unsafe { libc::localtime_r(&secs, &mut tm) };
    if converted.is_null() {
        return String::new();
    }
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        tm.tm_year as i64 + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min
    )
}

/// The session list row text: `[timestamp] title`.
pub fn session_label(session: &SessionInfo) -> String {
    format!(
        "[{}] {}",
        format_session_time(session.last_modified),
        session.title
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Local 2026-01-05 09:07 in milliseconds since epoch (the TS builds it with `new Date`).
    fn local() -> i64 {
        // SAFETY: zeroed `tm` is valid; `mktime` normalises and reads it.
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        tm.tm_year = 2026 - 1900;
        tm.tm_mon = 0;
        tm.tm_mday = 5;
        tm.tm_hour = 9;
        tm.tm_min = 7;
        tm.tm_sec = 0;
        tm.tm_isdst = -1;
        let secs = unsafe { libc::mktime(&mut tm) };
        (secs as i64) * 1000
    }

    #[test]
    fn format_session_time_pads_the_local_date_and_time() {
        assert_eq!(format_session_time(local()), "2026-01-05 09:07");
    }

    #[test]
    fn session_label_is_timestamp_then_title() {
        let info = SessionInfo {
            id: "a".into(),
            title: "Fix bug".into(),
            last_modified: local(),
        };
        assert_eq!(session_label(&info), "[2026-01-05 09:07] Fix bug");
    }

    #[test]
    fn serialises_with_camel_case_fields() {
        let info = SessionInfo {
            id: "a".into(),
            title: "t".into(),
            last_modified: 5,
        };
        let v = serde_json::to_value(&info).unwrap();
        assert_eq!(v["lastModified"], 5);
    }
}
