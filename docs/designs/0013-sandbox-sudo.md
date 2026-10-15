# 0013: Sandbox sudo

Status: Fulfilled

## Problem

Design 0008 kept the container unprivileged, so the agent could only do user-level installs. It could not install system packages (`apt`, PHP, Ruby).

## Decision

This reverses the "no sudo" decision of 0008.

- The image installs `sudo` with passwordless access for any uid.
- The container runs as the host uid, which has no passwd entry, and sudo refuses such uids. `/etc/passwd` is world-writable and the entrypoint script appends an entry at start. The password field is `*`, not `x`: with `x` PAM's account check looks for a `/etc/shadow` entry, finds none and sudo fails with "account validation failure".
- `build_run_args` drops `no-new-privileges` (it blocks setuid) and adds back `SETUID SETGID CHOWN DAC_OVERRIDE FOWNER FSETID KILL AUDIT_WRITE` after `--cap-drop ALL`.

- `sudo` resets `PATH` and `HOME`, so sudoers sets `secure_path` to include the user-level bin dirs. `HOME` becomes `/root` under sudo, which is fine for system installs.
- The entrypoint reports on stderr if it cannot write the passwd entry, instead of failing silently.

## Trade-offs

The agent can now act as root inside the container: it can write root-owned files into the project mount and modify any non-read-only file there. The `.git` read-only overlays still hold because remounting needs `CAP_SYS_ADMIN`, which stays dropped. Root in the container is not root on the host, but this is a weaker boundary than before.

## To do

- [x] Dockerfile and run args
- [x] Verified in a container with this setup (same caps, uid, `/etc/passwd`): `sudo apt-get update` and `install` work with the `*` entry and fail with `x`
- [x] Rebuilt image: `sudo apt-get update` and `install` work, and as root `.git` writes, `git commit` and a remount all fail
- [x] Run `cargo test` (556 passed)
- [x] Brief updated
- [x] `secure_path` and the entrypoint warning (sudoers syntax checked with `visudo -cf`; confirmed in the rebuilt image: `sudo sh -c 'echo $PATH'` lists the user-level bin dirs first)
