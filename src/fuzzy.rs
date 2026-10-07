//! Fuzzy matching for the command palette and file pickers (port of `src/shared/fuzzy.ts`).

/// Score how well `query` matches `text` as a case-insensitive subsequence, or `None` if it does
/// not. Higher is better: consecutive characters and matches at the start of a word score more.
pub fn fuzzy_score(query: &str, text: &str) -> Option<i64> {
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let mut score: i64 = 0;
    let mut from: usize = 0;
    let mut previous: i64 = -2;
    for ch in query.to_lowercase().chars() {
        let rest = t.get(from..)?;
        let index = rest.iter().position(|c| *c == ch)? + from;
        score += 1;
        if index as i64 == previous + 1 {
            score += 3;
        }
        if index == 0 || t[index - 1] == ' ' {
            score += 2;
        }
        previous = index as i64;
        from = index + 1;
    }
    Some(score)
}

/// Items matching `query`, best first (original order breaks ties); all of them for a blank query.
pub fn fuzzy_filter<T: Clone, F: Fn(&T) -> String>(items: &[T], query: &str, text: F) -> Vec<T> {
    let query = query.trim();
    if query.is_empty() {
        return items.to_vec();
    }
    let mut scored: Vec<(usize, i64, &T)> = Vec::new();
    for (index, item) in items.iter().enumerate() {
        if let Some(score) = fuzzy_score(query, &text(item)) {
            scored.push((index, score, item));
        }
    }
    scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    scored
        .into_iter()
        .map(|(_, _, item)| item.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> Vec<String> {
        vec![
            "Git: Pull".to_string(),
            "Git: Push".to_string(),
            "Git: Publish Branch".to_string(),
        ]
    }

    fn id(s: &String) -> String {
        s.clone()
    }

    #[test]
    fn score_is_none_when_the_characters_are_not_a_subsequence() {
        assert_eq!(fuzzy_score("xyz", "Git: Push"), None);
        assert_eq!(fuzzy_score("hsup", "push"), None);
    }

    #[test]
    fn score_matches_case_insensitively() {
        assert!(fuzzy_score("PUSH", "git: push").is_some());
    }

    #[test]
    fn score_is_higher_for_consecutive_and_word_start_matches() {
        let tight = fuzzy_score("pu", "Git: Pull").unwrap();
        let loose = fuzzy_score("pu", "Git: Stash Pop Undo").unwrap();
        assert!(tight > 0);
        assert!(fuzzy_score("gp", "Git: Pull").unwrap() > fuzzy_score("gp", "Digit grep").unwrap());
        assert!(tight >= loose);
    }

    #[test]
    fn filter_returns_everything_for_a_blank_query() {
        assert_eq!(fuzzy_filter(&items(), "  ", id), items());
    }

    #[test]
    fn filter_drops_non_matches_and_ranks_the_best_first() {
        assert_eq!(fuzzy_filter(&items(), "push", id)[0], "Git: Push");
        assert_eq!(fuzzy_filter(&items(), "zzz", id), Vec::<String>::new());
        assert_eq!(fuzzy_filter(&items(), "pub", id)[0], "Git: Publish Branch");
    }

    #[test]
    fn filter_keeps_the_original_order_for_equal_scores() {
        let both = vec!["b1".to_string(), "b2".to_string()];
        assert_eq!(fuzzy_filter(&both, "b", id), both);
    }
}
