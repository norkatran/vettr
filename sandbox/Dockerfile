# Sandbox image: Node, the Claude Agent SDK and the runner. Built by `npm run build:sandbox` in runner/,
# which bundles runner/dist/runner.mjs and runs `docker build -f ../sandbox/Dockerfile .` from runner/ (the
# build context is runner/, so the COPY path below is relative to it).
# glibc base (not alpine) because the SDK ships a native claude binary per platform.
FROM node:22-slim

# git for read-only commands (diff, log), ripgrep for the agent's search tools, and common toolchains
# (Python, a C/C++ build chain) and sudo so the agent can install further system packages
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        git ripgrep ca-certificates curl unzip sudo \
        python3 python3-pip python3-venv python-is-python3 \
        build-essential pkg-config \
    && rm -rf /var/lib/apt/lists/*

ARG SDK_VERSION
WORKDIR /opt/vettr
RUN npm init -y >/dev/null && npm install --omit=dev "@anthropic-ai/claude-agent-sdk@${SDK_VERSION}"

COPY dist/runner.mjs ./runner.mjs

# Passwordless sudo for whichever uid the container runs as. sudo refuses uids without a passwd entry and the
# host uid has none, so /etc/passwd is made writable and the entrypoint adds the entry at start. The password
# field must be `*`, not `x`: with `x` PAM looks for a shadow entry, finds none and sudo fails with "account
# validation failure". sudo needs setuid and a few capabilities, which `build_run_args` (src/sandbox.rs) grants
# back after `--cap-drop ALL`. sudo resets PATH, so secure_path keeps the user-level bin dirs visible.
RUN printf '%s\n' \
        'Defaults secure_path="/home/agent/.npm-global/bin:/home/agent/.local/bin:/home/agent/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"' \
        'ALL ALL=(ALL:ALL) NOPASSWD: ALL' > /etc/sudoers.d/agent \
    && chmod 0440 /etc/sudoers.d/agent \
    && chmod 666 /etc/passwd
RUN printf '%s\n' '#!/bin/sh' \
    'uid=$(id -u)' \
    'if ! getent passwd "$uid" >/dev/null 2>&1; then' \
    '    echo "agent:*:${uid}:$(id -g):agent:/home/agent:/bin/sh" >> /etc/passwd \' \
    '        || echo "vettr: could not add a passwd entry for uid ${uid}; sudo will not work" >&2' \
    'fi' \
    'exec node /opt/vettr/runner.mjs "$@"' > /opt/vettr/entrypoint.sh \
    && chmod +x /opt/vettr/entrypoint.sh

# The container runs as the host uid/gid (docker run --user), which has no passwd entry, so give it a real,
# world-writable home. User-level installs (npm -g, pip --user, cargo) go under it via the prefixes below.
RUN mkdir -p /home/agent && chmod 1777 /home/agent
ENV HOME=/home/agent \
    NPM_CONFIG_PREFIX=/home/agent/.npm-global \
    PIP_USER=1 \
    PIP_BREAK_SYSTEM_PACKAGES=1 \
    PYTHONUSERBASE=/home/agent/.local \
    CARGO_HOME=/home/agent/.cargo \
    PATH=/home/agent/.npm-global/bin:/home/agent/.local/bin:/home/agent/.cargo/bin:$PATH
ENTRYPOINT ["/opt/vettr/entrypoint.sh"]
