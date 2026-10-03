# Sandbox image: Node, the Claude Agent SDK and the runner (built by `npm run build:sandbox`).
# glibc base (not alpine) because the SDK ships a native claude binary per platform.
FROM node:22-slim

# git for read-only commands (diff, log) and ripgrep, which the agent's search tools use
RUN apt-get update \
    && apt-get install -y --no-install-recommends git ripgrep ca-certificates \
    && rm -rf /var/lib/apt/lists/*

ARG SDK_VERSION
WORKDIR /opt/agentide
RUN npm init -y >/dev/null && npm install --omit=dev "@anthropic-ai/claude-agent-sdk@${SDK_VERSION}"

COPY dist/runner.mjs ./runner.mjs

# The container runs as the host uid/gid (docker run --user), so give it a writable home at run time
ENV HOME=/tmp/home
ENTRYPOINT ["node", "/opt/agentide/runner.mjs"]
