//! Slash command completion for the composer (port of `src/shared/slashCommands.ts`).

use crate::agent::SlashCommandInfo;

/// The text after a leading slash when `text` is a command being typed (no whitespace yet), or
/// `None` once it is ordinary prose or the command has its arguments.
pub fn slash_query(text: &str) -> Option<String> {
    let rest = text.strip_prefix('/')?;
    if rest.chars().any(|c| c.is_whitespace()) {
        return None;
    }
    Some(rest.to_string())
}

fn names(command: &SlashCommandInfo) -> Vec<String> {
    let mut all = vec![command.name.clone()];
    if let Some(aliases) = &command.aliases {
        all.extend(aliases.iter().cloned());
    }
    all
}

fn rank(command: &SlashCommandInfo, query: &str) -> u8 {
    let lowered: Vec<String> = names(command).iter().map(|n| n.to_lowercase()).collect();
    if lowered.iter().any(|n| n.starts_with(query)) {
        0
    } else if lowered.iter().any(|n| n.contains(query)) {
        1
    } else {
        2
    }
}

/// Commands matching what has been typed: name prefixes first, then other substrings.
pub fn filter_commands(commands: &[SlashCommandInfo], query: &str) -> Vec<SlashCommandInfo> {
    let q = query.to_lowercase();
    let mut ranked: Vec<(u8, &SlashCommandInfo)> = commands
        .iter()
        .map(|c| (rank(c, &q), c))
        .filter(|(r, _)| *r < 2)
        .collect();
    // Stable sort: equal names keep their original order, like the TS.
    ranked.sort_by(|a, b| {
        a.0.cmp(&b.0).then_with(|| {
            let (x, y) = (a.1.name.to_lowercase(), b.1.name.to_lowercase());
            x.cmp(&y).then_with(|| a.1.name.cmp(&b.1.name))
        })
    });
    ranked.into_iter().map(|(_, c)| c.clone()).collect()
}

/// The composer text after choosing `command`: ready for its arguments.
pub fn complete_command(command: &SlashCommandInfo) -> String {
    format!("/{} ", command.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(name: &str, aliases: Option<Vec<&str>>) -> SlashCommandInfo {
        SlashCommandInfo {
            name: name.to_string(),
            description: String::new(),
            argument_hint: String::new(),
            aliases: aliases.map(|a| a.into_iter().map(|s| s.to_string()).collect()),
        }
    }

    fn all() -> Vec<SlashCommandInfo> {
        vec![
            cmd("review", None),
            cmd("init", None),
            cmd("preview", None),
            cmd("usage", Some(vec!["cost"])),
        ]
    }

    fn names_of(list: &[SlashCommandInfo]) -> Vec<String> {
        list.iter().map(|c| c.name.clone()).collect()
    }

    #[test]
    fn slash_query_reads_a_command_being_typed() {
        assert_eq!(slash_query("/"), Some(String::new()));
        assert_eq!(slash_query("/rev"), Some("rev".to_string()));
    }

    #[test]
    fn slash_query_ignores_prose_arguments_and_multi_line_text() {
        assert_eq!(slash_query("fix /rev"), None);
        assert_eq!(slash_query("/review file.ts"), None);
        assert_eq!(slash_query("/review\nmore"), None);
        assert_eq!(slash_query(""), None);
    }

    #[test]
    fn filter_lists_everything_for_an_empty_query() {
        assert_eq!(filter_commands(&all(), "").len(), 4);
    }

    #[test]
    fn filter_puts_prefix_matches_before_substring_matches() {
        assert_eq!(
            names_of(&filter_commands(&all(), "rev")),
            vec!["review".to_string(), "preview".to_string()]
        );
    }

    #[test]
    fn filter_matches_aliases_and_ignores_case() {
        assert_eq!(
            names_of(&filter_commands(&all(), "COS")),
            vec!["usage".to_string()]
        );
    }

    #[test]
    fn filter_returns_nothing_when_nothing_matches() {
        assert_eq!(
            filter_commands(&all(), "zzz"),
            Vec::<SlashCommandInfo>::new()
        );
    }

    #[test]
    fn complete_command_adds_the_slash_and_a_space_for_arguments() {
        assert_eq!(complete_command(&cmd("init", None)), "/init ");
    }
}
