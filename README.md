# agentide

Agent-first review IDE (pronounced like "agentic"). See [docs/PROJECT_BRIEF.md](docs/PROJECT_BRIEF.md) for the concept, decisions and build order.

Electron + TypeScript + React, built with [electron-vite](https://electron-vite.org).

## Layout

- `src/main` - main process (filesystem, git, agent processes)
- `src/preload` - context-isolated bridge exposing `window.agentide`
- `src/renderer` - React UI
- `src/shared` - types shared across processes (typed IPC contract)

## Scripts

- `npm run dev` - run with hot reload
- `npm run build` - production build into `out/`
- `npm start` - preview the production build
- `npm run typecheck` - type-check main/preload and renderer

If the Electron binary is missing after `npm install` (install scripts disabled), run `node node_modules/electron/install.js`.
