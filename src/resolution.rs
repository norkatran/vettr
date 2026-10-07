//! Resolving a comment resolves its whole thread (the comment and the agent's replies to it), as on
//! GitHub or GitLab. Only the user resolves: the agent's "resolved" reply is advisory. The ids of
//! the resolved comments are kept per project by the app (comment ids are UUIDs, unique across
//! sessions). Port of `src/shared/resolution.ts`.

use serde_json::Value;

/// The resolved ids from stored data; anything that is not a list of strings is dropped.
pub fn parse_resolved(raw: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Value::Array(list) = raw {
        for item in list {
            if let Value::String(id) = item {
                if !id.is_empty() && !out.contains(id) {
                    out.push(id.clone());
                }
            }
        }
    }
    out
}

/// `ids` with `id` marked resolved or reopened.
pub fn with_resolved(ids: &[String], id: &str, resolved: bool) -> Vec<String> {
    let mut out: Vec<String> = ids.to_vec();
    with_resolved_in_place(&mut out, id, resolved);
    out
}

/// Mark `id` resolved or reopened in place; returns whether the list changed.
pub fn with_resolved_in_place(ids: &mut Vec<String>, id: &str, resolved: bool) -> bool {
    let has = ids.iter().any(|other| other == id);
    if has == resolved {
        return false;
    }
    if resolved {
        ids.push(id.to_string());
    } else {
        ids.retain(|other| other != id);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_keeps_the_string_ids_once_each() {
        assert_eq!(
            parse_resolved(&json!(["a", "b", "a"])),
            strings(&["a", "b"])
        );
    }

    #[test]
    fn parse_drops_anything_else() {
        assert_eq!(parse_resolved(&json!(["a", 1, null, ""])), strings(&["a"]));
        assert_eq!(
            parse_resolved(&json!({"not": "a list"})),
            Vec::<String>::new()
        );
        assert_eq!(parse_resolved(&Value::Null), Vec::<String>::new());
    }

    #[test]
    fn adds_and_removes_an_id() {
        assert_eq!(
            with_resolved(&strings(&["a"]), "b", true),
            strings(&["a", "b"])
        );
        assert_eq!(
            with_resolved(&strings(&["a", "b"]), "a", false),
            strings(&["b"])
        );
    }

    #[test]
    fn reports_no_change_when_nothing_changes() {
        let mut ids = strings(&["a"]);
        assert!(!with_resolved_in_place(&mut ids, "a", true));
        assert!(!with_resolved_in_place(&mut ids, "z", false));
        assert_eq!(ids, strings(&["a"]));
        assert!(with_resolved_in_place(&mut ids, "z", true));
    }
}
