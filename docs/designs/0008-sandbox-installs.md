# 0008 Sandbox installs

Status: Fulfilled

## Problem

The agent runs unprivileged (host uid/gid, all capabilities dropped, `no-new-privileges`), so it could not install anything: system paths are read-only for it, and `HOME=/tmp/home` was never created.

## Decision

- Create a real, world-writable `/home/agent` in the image and use it as `HOME` (no tmpfs; it lives and dies with the container).
- Point user-level install locations at it: `NPM_CONFIG_PREFIX`, `PIP_USER` (with `PIP_BREAK_SYSTEM_PACKAGES`), `PYTHONUSERBASE`, `CARGO_HOME`, and add their bin dirs to `PATH`.
- Bake in common toolchains: `curl`, `unzip`, Python 3 with pip and venv, `build-essential`, `pkg-config`.
- Keep the container unprivileged. No root, no `sudo`, no relaxed capabilities.

## Out of scope

Persisting installs between runs, per-project image overrides, root-style `apt` installs.

## To do

- [x] Dockerfile: home directory, install prefixes, toolchains
- [x] Update the brief
- [x] Rebuild the image (`npm run build:sandbox` in `runner/`) and verify `npm i -g`, `pip install` and `cargo` work
