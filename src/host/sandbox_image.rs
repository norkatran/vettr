//! Building the sandbox image (port of `src/main/sandboxImage.ts`).
//!
//! The runner now lives in `runner/` and the Dockerfile in `sandbox/`, so the Dockerfile and the
//! build context (the runner directory, which holds `dist/runner.mjs`) are separate parameters.

use std::io::Read;
use std::sync::mpsc::channel;
use std::thread;

use crate::host::sandbox_runtime::{tail_chars, Spawn, Utf8Decoder};

pub struct BuildImageOptions<'a> {
    pub spawn: &'a Spawn,
    /// Path of the Dockerfile (`sandbox/Dockerfile`).
    pub dockerfile: String,
    /// The build context: the runner directory (it holds `dist/runner.mjs`).
    pub context_dir: String,
    pub image: String,
    pub sdk_version: String,
    /// Called with the latest line of build output.
    pub on_progress: &'a dyn Fn(&str),
}

const OUTPUT_TAIL: usize = 2000;
const MAX_PROGRESS: usize = 100;

/// Build args for `docker build` with plain, line-based progress output.
pub fn build_image_args(
    image: &str,
    sdk_version: &str,
    dockerfile: &str,
    context_dir: &str,
) -> Vec<String> {
    vec![
        "build".to_string(),
        "--progress=plain".to_string(),
        "-t".to_string(),
        image.to_string(),
        "--build-arg".to_string(),
        format!("SDK_VERSION={}", sdk_version),
        "-f".to_string(),
        dockerfile.to_string(),
        context_dir.to_string(),
    ]
}

/// A build log line made short enough for a status message, or `None` when it is blank.
pub fn progress_line(raw: &str) -> Option<String> {
    let line = raw.trim();
    if line.is_empty() {
        return None;
    }
    if line.chars().count() > MAX_PROGRESS {
        let cut: String = line.chars().take(MAX_PROGRESS - 1).collect();
        return Some(format!("{}\u{2026}", cut));
    }
    Some(line.to_string())
}

fn read_stream(
    stream: Option<Box<dyn Read + Send>>,
    index: usize,
    tx: std::sync::mpsc::Sender<(usize, String)>,
) {
    thread::spawn(move || {
        if let Some(mut reader) = stream {
            let mut decoder = Utf8Decoder::default();
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let text = decoder.push(&buf[..n]);
                        if !text.is_empty() && tx.send((index, text)).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    });
}

/// Build the sandbox image. Returns `Ok(())` on success or a message for the user, which
/// includes the end of Docker's output when the build fails. Blocks until the build ends.
pub fn build_sandbox_image(options: BuildImageOptions) -> Result<(), String> {
    let args = build_image_args(
        &options.image,
        &options.sdk_version,
        &options.dockerfile,
        &options.context_dir,
    );
    let child = match (options.spawn)("docker", &args) {
        Ok(child) => child,
        Err(error) => {
            return Err(format!(
                "Could not run Docker to build the sandbox image: {}",
                error
            ))
        }
    };
    let (tx, rx) = channel::<(usize, String)>();
    read_stream(child.take_stdout(), 0, tx.clone());
    read_stream(child.take_stderr(), 1, tx.clone());
    drop(tx);

    let mut tail = String::new();
    let mut pending: [String; 2] = [String::new(), String::new()];
    // Ends once both streams reach end of file.
    while let Ok((index, text)) = rx.recv() {
        tail = tail_chars(&format!("{}{}", tail, text), OUTPUT_TAIL);
        let joined = format!("{}{}", pending[index], text);
        let mut parts: Vec<String> = joined.split('\n').map(|p| p.to_string()).collect();
        pending[index] = parts.pop().unwrap_or_default();
        for part in parts {
            if let Some(line) = progress_line(&part) {
                (options.on_progress)(&line);
            }
        }
    }
    match child.wait() {
        Err(error) => Err(format!(
            "Could not run Docker to build the sandbox image: {}",
            error
        )),
        Ok(Some(0)) => Ok(()),
        Ok(code) => {
            let shown = match code {
                Some(c) => c.to_string(),
                None => "terminated by a signal".to_string(),
            };
            Err(format!(
                "Building the sandbox image failed (exit code {}).\n{}",
                shown,
                tail.trim()
            )
            .trim()
            .to_string())
        }
    }
}

/// The Agent SDK version the image must match, read from `runner/package.json`
/// (`devDependencies`), or `None` when it cannot be read. A leading `^` or `~` is dropped.
pub fn read_sdk_version(
    package_json_path: &str,
    read_file: &dyn Fn(&str) -> Result<String, String>,
) -> Option<String> {
    let text = read_file(package_json_path).ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&text).ok()?;
    let version = manifest
        .get("devDependencies")?
        .get("@anthropic-ai/claude-agent-sdk")?
        .as_str()?;
    let version = version.trim().trim_start_matches(['^', '~']);
    if version.is_empty() {
        None
    } else {
        Some(version.to_string())
    }
}

pub struct EnsureImageOptions<'a> {
    pub spawn: &'a Spawn,
    /// Path of the Dockerfile (`sandbox/Dockerfile`).
    pub dockerfile: String,
    /// The runner directory (`runner/`), holding `package.json` and `dist/runner.mjs`.
    pub runner_dir: String,
    pub image: String,
    pub exists: &'a dyn Fn(&str) -> bool,
    pub read_file: &'a dyn Fn(&str) -> Result<String, String>,
    pub on_progress: &'a dyn Fn(&str),
}

/// Build the image from the app's own `runner/` and `sandbox/`. Returns `None` when that is not
/// possible here (no bundled runner or Dockerfile, or the SDK version is unknown), so the caller
/// falls back to telling the user to run `npm run build:sandbox`.
pub fn build_image_from_app(options: EnsureImageOptions) -> Option<Result<(), String>> {
    let bundle = format!("{}/dist/runner.mjs", options.runner_dir);
    if !(options.exists)(&bundle) || !(options.exists)(&options.dockerfile) {
        return None;
    }
    let manifest = format!("{}/package.json", options.runner_dir);
    let sdk_version = read_sdk_version(&manifest, options.read_file)?;
    Some(build_sandbox_image(BuildImageOptions {
        spawn: options.spawn,
        dockerfile: options.dockerfile.clone(),
        context_dir: options.runner_dir.clone(),
        image: options.image.clone(),
        sdk_version,
        on_progress: options.on_progress,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::sandbox_runtime::testing::*;
    use std::cell::RefCell;
    use std::sync::Arc;

    fn args(image: &str, version: &str) -> Vec<String> {
        build_image_args(image, version, "/df/Dockerfile", "/ctx")
    }

    #[test]
    fn build_image_args_pass_the_sdk_version_and_build_from_the_context_directory() {
        let expected: Vec<String> = [
            "build",
            "--progress=plain",
            "-t",
            "img",
            "--build-arg",
            "SDK_VERSION=1.2.3",
            "-f",
            "/df/Dockerfile",
            "/ctx",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(args("img", "1.2.3"), expected);
    }

    #[test]
    fn progress_line_trims_skips_blanks_and_shortens_long_lines() {
        assert_eq!(
            progress_line("  #5 RUN npm install  "),
            Some("#5 RUN npm install".to_string())
        );
        assert_eq!(progress_line("   "), None);
        let long = "x".repeat(150);
        assert_eq!(
            progress_line(&long),
            Some(format!("{}\u{2026}", "x".repeat(99)))
        );
    }

    fn run_build(
        process: Arc<FakeProcess>,
        progress: &RefCell<Vec<String>>,
    ) -> (Result<(), String>, Vec<(String, Vec<String>)>) {
        let (spawn, calls) = fake_spawn(process);
        let on_progress = |line: &str| progress.borrow_mut().push(line.to_string());
        let result = build_sandbox_image(BuildImageOptions {
            spawn: &spawn,
            dockerfile: "/df/Dockerfile".to_string(),
            context_dir: "/ctx".to_string(),
            image: "img".to_string(),
            sdk_version: "1.0.0".to_string(),
            on_progress: &on_progress,
        });
        let recorded = calls.lock().unwrap().clone();
        (result, recorded)
    }

    #[test]
    fn build_reports_progress_lines_from_both_streams_and_succeeds() {
        let process = FakeProcess::new();
        process.stdout_write("#1 [1/3] FROM node\n#2 split ");
        process.stdout_write("line\n\n");
        process.stderr_write("#3 from stderr\n");
        process.stdout_write("partial without newline");
        process.close(Some(0));
        let progress = RefCell::new(Vec::new());
        let (result, calls) = run_build(process, &progress);
        assert_eq!(result, Ok(()));
        let seen = progress.borrow().clone();
        // The two streams are read on separate threads, so only the order within a stream is fixed.
        assert_eq!(seen.len(), 3);
        assert!(seen.contains(&"#1 [1/3] FROM node".to_string()));
        assert!(seen.contains(&"#2 split line".to_string()));
        assert!(seen.contains(&"#3 from stderr".to_string()));
        assert!(!seen.contains(&"partial without newline".to_string()));
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "docker");
        assert_eq!(calls[0].1, args("img", "1.0.0"));
    }

    #[test]
    fn build_includes_the_end_of_the_output_when_it_fails() {
        let process = FakeProcess::new();
        process.stderr_write("E: package not found\n");
        process.close(Some(100));
        let progress = RefCell::new(Vec::new());
        let (result, _) = run_build(process, &progress);
        assert_eq!(
            result,
            Err(
                "Building the sandbox image failed (exit code 100).\nE: package not found"
                    .to_string()
            )
        );
    }

    #[test]
    fn build_keeps_only_the_tail_of_very_long_output() {
        let process = FakeProcess::new();
        process.stdout_write(&format!("{}END\n", "a".repeat(5000)));
        process.close(Some(1));
        let progress = RefCell::new(Vec::new());
        let (result, _) = run_build(process, &progress);
        let message = result.unwrap_err();
        assert!(message.ends_with("END"));
        assert!(message.len() < 2100);
    }

    #[test]
    fn build_reports_docker_failing_to_run() {
        let process = FakeProcess::new();
        process.close_error("ENOENT");
        let progress = RefCell::new(Vec::new());
        let (result, _) = run_build(process, &progress);
        assert!(result.unwrap_err().contains("ENOENT"));
    }

    #[test]
    fn build_reports_a_spawn_failure() {
        let spawn: Spawn = Box::new(|_: &str, _: &[String]| Err("no such file".to_string()));
        let on_progress = |_: &str| {};
        let result = build_sandbox_image(BuildImageOptions {
            spawn: &spawn,
            dockerfile: "d".to_string(),
            context_dir: "c".to_string(),
            image: "img".to_string(),
            sdk_version: "1".to_string(),
            on_progress: &on_progress,
        });
        assert!(result.unwrap_err().contains("no such file"));
    }

    // readSdkVersion

    const PATH: &str = "/app/runner/package.json";

    #[test]
    fn read_sdk_version_reads_the_dev_dependency() {
        let seen: RefCell<Vec<String>> = RefCell::new(Vec::new());
        let read = |path: &str| -> Result<String, String> {
            seen.borrow_mut().push(path.to_string());
            Ok(r#"{"devDependencies":{"@anthropic-ai/claude-agent-sdk":"0.3.288"}}"#.to_string())
        };
        assert_eq!(read_sdk_version(PATH, &read), Some("0.3.288".to_string()));
        assert_eq!(seen.borrow().clone(), vec![PATH.to_string()]);
        let ranged = |_: &str| -> Result<String, String> {
            Ok(r#"{"devDependencies":{"@anthropic-ai/claude-agent-sdk":"^0.3.288"}}"#.to_string())
        };
        assert_eq!(read_sdk_version(PATH, &ranged), Some("0.3.288".to_string()));
    }

    #[test]
    fn read_sdk_version_is_none_when_unreadable_malformed_or_without_a_version() {
        let unreadable = |_: &str| -> Result<String, String> { Err("x".to_string()) };
        assert_eq!(read_sdk_version(PATH, &unreadable), None);
        let malformed = |_: &str| -> Result<String, String> { Ok("not json".to_string()) };
        assert_eq!(read_sdk_version(PATH, &malformed), None);
        let not_a_string = |_: &str| -> Result<String, String> {
            Ok(r#"{"devDependencies":{"@anthropic-ai/claude-agent-sdk":3}}"#.to_string())
        };
        assert_eq!(read_sdk_version(PATH, &not_a_string), None);
        let missing = |_: &str| -> Result<String, String> { Ok("{}".to_string()) };
        assert_eq!(read_sdk_version(PATH, &missing), None);
    }

    // buildImageFromApp

    #[test]
    fn build_from_app_builds_from_the_runner_directory() {
        let process = FakeProcess::new();
        process.close(Some(0));
        let (spawn, calls) = fake_spawn(process);
        let exists = |_: &str| true;
        let read = |_: &str| -> Result<String, String> {
            Ok(r#"{"devDependencies":{"@anthropic-ai/claude-agent-sdk":"9.9.9"}}"#.to_string())
        };
        let on_progress = |_: &str| {};
        let result = build_image_from_app(EnsureImageOptions {
            spawn: &spawn,
            dockerfile: "/app/sandbox/Dockerfile".to_string(),
            runner_dir: "/app/runner".to_string(),
            image: "img".to_string(),
            exists: &exists,
            read_file: &read,
            on_progress: &on_progress,
        });
        assert_eq!(result, Some(Ok(())));
        let calls = calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "docker");
        assert_eq!(
            calls[0].1,
            build_image_args("img", "9.9.9", "/app/sandbox/Dockerfile", "/app/runner")
        );
    }

    #[test]
    fn build_from_app_does_nothing_without_the_bundled_runner_or_the_sdk_version() {
        let (spawn, calls) = fake_spawn(FakeProcess::new());
        let on_progress = |_: &str| {};
        let empty = |_: &str| -> Result<String, String> { Ok("{}".to_string()) };
        let no = |_: &str| false;
        let yes = |_: &str| true;
        let result = build_image_from_app(EnsureImageOptions {
            spawn: &spawn,
            dockerfile: "/app/sandbox/Dockerfile".to_string(),
            runner_dir: "/app/runner".to_string(),
            image: "img".to_string(),
            exists: &no,
            read_file: &empty,
            on_progress: &on_progress,
        });
        assert_eq!(result, None);
        let result = build_image_from_app(EnsureImageOptions {
            spawn: &spawn,
            dockerfile: "/app/sandbox/Dockerfile".to_string(),
            runner_dir: "/app/runner".to_string(),
            image: "img".to_string(),
            exists: &yes,
            read_file: &empty,
            on_progress: &on_progress,
        });
        assert_eq!(result, None);
        assert!(calls.lock().unwrap().is_empty());
    }
}
