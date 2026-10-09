//! Command-line arguments (design 0012): `vettr [OPTIONS] [PROJECT]`.

use std::path::{Path, PathBuf};

use crate::profiles::ProfileInfo;

pub const USAGE: &str = "\
Usage: vettr [OPTIONS] [PROJECT]

Arguments:
  [PROJECT]  Folder to open; a folder inside a git repository opens that repository

Options:
  -p, --profile <NAME|ID>  Credential profile to use for this launch (not remembered)
  -h, --help               Print this help
  -V, --version            Print the version";

/// What the user asked for at launch.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LaunchOptions {
    /// The project folder, made absolute against the working directory.
    pub project: Option<String>,
    /// A profile name (case-insensitive) or id.
    pub profile: Option<String>,
}

#[derive(Debug, PartialEq)]
pub enum Parsed {
    Run(LaunchOptions),
    /// Print this text to stdout and exit successfully.
    Print(String),
}

/// Parse the arguments after the program name. `cwd` resolves a relative project path.
/// Errors are messages for stderr.
pub fn parse<I, S>(args: I, cwd: &Path) -> Result<Parsed, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut args = args.into_iter().map(Into::into);
    let mut options = LaunchOptions::default();
    let mut only_positional = false;
    while let Some(arg) = args.next() {
        if only_positional || arg == "-" || !arg.starts_with('-') {
            set_project(&mut options, &arg, cwd)?;
            continue;
        }
        match arg.as_str() {
            "--" => only_positional = true,
            "-h" | "--help" => return Ok(Parsed::Print(USAGE.to_string())),
            "-V" | "--version" => {
                return Ok(Parsed::Print(format!(
                    "vettr {}",
                    env!("CARGO_PKG_VERSION")
                )))
            }
            "-p" | "--profile" => {
                let value = args.next().ok_or("--profile needs a value")?;
                set_profile(&mut options, value)?;
            }
            _ => match arg.strip_prefix("--profile=") {
                Some(value) => set_profile(&mut options, value.to_string())?,
                None => return Err(format!("unknown option '{arg}'")),
            },
        }
    }
    Ok(Parsed::Run(options))
}

fn set_project(options: &mut LaunchOptions, arg: &str, cwd: &Path) -> Result<(), String> {
    if options.project.is_some() {
        return Err("only one project can be opened".to_string());
    }
    if arg.is_empty() {
        return Err("the project path is empty".to_string());
    }
    let path: PathBuf = cwd.join(arg); // an absolute `arg` replaces `cwd`
    options.project = Some(path.to_string_lossy().to_string());
    Ok(())
}

fn set_profile(options: &mut LaunchOptions, value: String) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err("--profile needs a value".to_string());
    }
    options.profile = Some(value);
    Ok(())
}

/// The id of the profile `wanted` names: an exact id first, then a name ignoring case.
pub fn find_profile(profiles: &[ProfileInfo], wanted: &str) -> Option<String> {
    let wanted = wanted.trim();
    profiles
        .iter()
        .find(|p| p.id == wanted)
        .or_else(|| {
            let lowered = wanted.to_lowercase();
            profiles.iter().find(|p| p.name.to_lowercase() == lowered)
        })
        .map(|p| p.id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> Result<Parsed, String> {
        parse(args.iter().copied(), Path::new("/work"))
    }

    fn opts(project: Option<&str>, profile: Option<&str>) -> Parsed {
        Parsed::Run(LaunchOptions {
            project: project.map(String::from),
            profile: profile.map(String::from),
        })
    }

    fn info(id: &str, name: &str) -> ProfileInfo {
        ProfileInfo {
            id: id.to_string(),
            name: name.to_string(),
        }
    }

    #[test]
    fn no_arguments_changes_nothing() {
        assert_eq!(run(&[]), Ok(opts(None, None)));
    }

    #[test]
    fn a_positional_project_is_resolved_against_the_working_directory() {
        assert_eq!(run(&["repo"]), Ok(opts(Some("/work/repo"), None)));
        assert_eq!(run(&["."]), Ok(opts(Some("/work/."), None)));
        assert_eq!(run(&["/abs/repo"]), Ok(opts(Some("/abs/repo"), None)));
    }

    #[test]
    fn the_profile_can_come_before_or_after_the_project() {
        let expected = Ok(opts(Some("/work/repo"), Some("Work")));
        assert_eq!(run(&["-p", "Work", "repo"]), expected);
        assert_eq!(run(&["repo", "--profile", "Work"]), expected);
        assert_eq!(run(&["--profile=Work", "repo"]), expected);
    }

    #[test]
    fn double_dash_allows_a_project_starting_with_a_dash() {
        assert_eq!(run(&["--", "-odd"]), Ok(opts(Some("/work/-odd"), None)));
    }

    #[test]
    fn help_and_version_print() {
        assert_eq!(run(&["-h"]), Ok(Parsed::Print(USAGE.to_string())));
        assert_eq!(
            run(&["repo", "--help"]),
            Ok(Parsed::Print(USAGE.to_string()))
        );
        match run(&["--version"]) {
            Ok(Parsed::Print(text)) => assert!(text.starts_with("vettr ")),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn bad_input_is_an_error() {
        assert!(run(&["--nope"]).unwrap_err().contains("unknown option"));
        assert!(run(&["-p"]).unwrap_err().contains("needs a value"));
        assert!(run(&["--profile="]).unwrap_err().contains("needs a value"));
        assert!(run(&["a", "b"]).unwrap_err().contains("only one project"));
        assert!(run(&[""]).unwrap_err().contains("empty"));
    }

    #[test]
    fn finds_a_profile_by_id_or_name_ignoring_case() {
        let profiles = vec![info("id-1", "Work"), info("id-2", "Personal")];
        assert_eq!(find_profile(&profiles, "id-2"), Some("id-2".to_string()));
        assert_eq!(find_profile(&profiles, " work "), Some("id-1".to_string()));
        assert_eq!(find_profile(&profiles, "other"), None);
    }
}
