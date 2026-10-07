# Sandbox image: Node, the Claude Agent SDK and the runner. Built by `npm run build:sandbox` in runner/,
# which bundles runner/dist/runner.mjs and runs `docker build -f ../sandbox/Dockerfile .` from runner/ (the
# build context is runner/, so the COPY path below is relative to it).
# glibc base (not alpine) because the SDK ships a native claude binary per platform.
FROM node:22-slim

# git for read-only commands (diff, log), ripgrep for the agent's search tools, and common toolchains
# (Python, a C/C++ build chain) so the agent need not install system packages, which it cannot do as non-root
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        git ripgrep ca-certificates curl unzip \
        python3 python3-pip python3-venv python-is-python3 \
        build-essential pkg-config \
    && rm -rf /var/lib/apt/lists/*

ARG SDK_VERSION
WORKDIR /opt/vettr
RUN npm init -y >/dev/null && npm install --omit=dev "@anthropic-ai/claude-agent-sdk@${SDK_VERSION}"

COPY dist/runner.mjs ./runner.mjs

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
ENTRYPOINT ["node", "/opt/vettr/runner.mjs"]
