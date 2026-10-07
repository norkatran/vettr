//! Arguments for the sandbox container (port of `src/shared/sandbox.ts`).

use serde::{Deserialize, Serialize};

pub const SANDBOX_IMAGE: &str = "vettr-sandbox";

/// Label put on every container vettr starts, so leftovers can be found and removed.
pub const CONTAINER_LABEL: &str = "vettr.app=1";
/// Label holding the pid of the vettr process that started the container.
pub const OWNER_LABEL: &str = "vettr.pid";

/// Where the transcripts dir is mounted inside the container; only this dir of the config is shared.
pub const CONTAINER_CONFIG_DIR: &str = "/vettr-config";

#[derive(Debug, Clone, PartialEq)]
pub struct RunOptions {
    pub name: String,
    /// Pid of the app process that owns the container (see `OWNER_LABEL`).
    pub owner_pid: u32,
    /// Absolute project root; mounted at the same path inside the container so paths line up.
    pub project: String,
    /// Absolute paths to mount read-only over the project (git dirs and a `.git` file).
    pub read_only_paths: Vec<String>,
    /// Host dir mounted as the container's Claude config dir, so SDK transcripts outlive it.
    pub transcripts_dir: String,
    pub uid: u32,
    pub gid: u32,
}

/// Arguments for `docker run` (after the `docker` command). The container runs as the host user
/// so files are not root-owned, drops all capabilities, and keeps stdin open for the protocol.
/// Read-only mounts come after the project mount so they overlay it. The runner is the image
/// entrypoint, so nothing follows the image name.
pub fn build_run_args(opts: &RunOptions) -> Vec<String> {
    let mut seen: Vec<&String> = Vec::new();
    for p in &opts.read_only_paths {
        if !seen.contains(&p) {
            seen.push(p);
        }
    }
    let mut args: Vec<String> = Vec::new();
    let mut push = |items: &[&str]| {
        for item in items {
            args.push(item.to_string());
        }
    };
    push(&["run", "--rm", "-i", "--init", "--name"]);
    push(&[opts.name.as_str()]);
    push(&["--label", CONTAINER_LABEL, "--label"]);
    let owner = format!("{}={}", OWNER_LABEL, opts.owner_pid);
    push(&[owner.as_str(), "--user"]);
    let user = format!("{}:{}", opts.uid, opts.gid);
    push(&[user.as_str()]);
    push(&["--cap-drop", "ALL", "--security-opt", "no-new-privileges"]);
    // Without this, SELinux hosts (Fedora, RHEL) deny the container access to the bind mounts.
    // The alternative, relabelling the mounts with :z, would change labels on the user's files.
    push(&["--security-opt", "label=disable", "-v"]);
    let config_mount = format!("{}:{}", opts.transcripts_dir, CONTAINER_CONFIG_DIR);
    push(&[config_mount.as_str(), "-e"]);
    let config_env = format!("CLAUDE_CONFIG_DIR={}", CONTAINER_CONFIG_DIR);
    push(&[config_env.as_str(), "--workdir"]);
    push(&[opts.project.as_str(), "-v"]);
    let project_mount = format!("{}:{}", opts.project, opts.project);
    push(&[project_mount.as_str()]);
    for path in seen {
        let mount = format!("{}:{}:ro", path, path);
        push(&["-v", mount.as_str()]);
    }
    push(&[SANDBOX_IMAGE]);
    args
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrphanCandidate {
    pub id: String,
    pub pid: Option<u32>,
}

/// One line of `docker ps` output for the sweep: container id and owner pid (may be empty).
pub fn parse_orphan_candidates(output: &str) -> Vec<OrphanCandidate> {
    let mut out: Vec<OrphanCandidate> = Vec::new();
    for line in output.split('\n') {
        let mut parts = line.split_whitespace();
        let id = match parts.next() {
            Some(id) => id,
            None => continue,
        };
        let pid: Option<u32> = match parts.next() {
            Some(p) if p.bytes().all(|b| b.is_ascii_digit()) => p.parse::<u32>().ok(),
            _ => None,
        };
        out.push(OrphanCandidate {
            id: id.to_string(),
            pid,
        });
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DockerProblemKind {
    Docker,
    Image,
}

/// Why the sandbox cannot start: Docker itself, or only its image (which the app can build).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DockerProblem {
    pub kind: DockerProblemKind,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> RunOptions {
        RunOptions {
            name: "c1".to_string(),
            owner_pid: 4242,
            project: "/work/p".to_string(),
            read_only_paths: Vec::new(),
            transcripts_dir: "/data/t".to_string(),
            uid: 1000,
            gid: 1001,
        }
    }

    /// The values that directly follow `flag`.
    fn after(args: &[String], flag: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for i in 1..args.len() {
            if args[i - 1] == flag {
                out.push(args[i].clone());
            }
        }
        out
    }

    #[test]
    fn runs_as_the_host_user_with_the_project_mounted_at_the_same_path() {
        let args = build_run_args(&base());
        assert_eq!(&args[..3], &["run", "--rm", "-i"]);
        assert!(args.contains(&"--init".to_string()));
        let joined = args.join(" ");
        assert!(joined.contains("--name c1"));
        assert!(joined.contains("--user 1000:1001"));
        assert!(joined.contains("--workdir /work/p"));
        assert!(joined.contains("-v /work/p:/work/p"));
    }

    #[test]
    fn mounts_the_transcripts_dir_as_the_claude_config_dir() {
        let joined = build_run_args(&base()).join(" ");
        assert!(joined.contains("-v /data/t:/vettr-config"));
        assert!(joined.contains("-e CLAUDE_CONFIG_DIR=/vettr-config"));
    }

    #[test]
    fn drops_capabilities_and_ends_with_the_image_so_the_runner_gets_no_extra_args() {
        let args = build_run_args(&base());
        let joined = args.join(" ");
        assert!(joined.contains("--cap-drop ALL"));
        assert!(joined.contains("--security-opt no-new-privileges"));
        assert!(joined.contains("--security-opt label=disable"));
        assert_eq!(args.last().map(|s| s.as_str()), Some(SANDBOX_IMAGE));
    }

    #[test]
    fn mounts_read_only_paths_after_the_project_mount_without_duplicates() {
        let mut opts = base();
        opts.read_only_paths = vec![
            "/work/p/.git".to_string(),
            "/work/p/.git".to_string(),
            "/main/.git/worktrees/x".to_string(),
        ];
        let args = build_run_args(&opts);
        assert_eq!(
            after(&args, "-v"),
            vec![
                "/data/t:/vettr-config".to_string(),
                "/work/p:/work/p".to_string(),
                "/work/p/.git:/work/p/.git:ro".to_string(),
                "/main/.git/worktrees/x:/main/.git/worktrees/x:ro".to_string(),
            ]
        );
    }

    #[test]
    fn labels_the_container_with_the_app_and_its_owner_pid() {
        let args = build_run_args(&base());
        assert_eq!(
            after(&args, "--label"),
            vec!["vettr.app=1".to_string(), "vettr.pid=4242".to_string()]
        );
    }

    #[test]
    fn parse_orphan_candidates_reads_ids_with_their_owner_pid() {
        assert_eq!(
            parse_orphan_candidates("abc 12\ndef\n\nghi x\n"),
            vec![
                OrphanCandidate {
                    id: "abc".to_string(),
                    pid: Some(12)
                },
                OrphanCandidate {
                    id: "def".to_string(),
                    pid: None
                },
                OrphanCandidate {
                    id: "ghi".to_string(),
                    pid: None
                },
            ]
        );
    }
}
