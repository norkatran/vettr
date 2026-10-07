# vettr runner

The sandbox runner: a small Node program that runs inside the vettr Docker container, wraps the
Claude Agent SDK and talks to the host app over JSON lines on stdio (`src/agent.ts` defines the
protocol; the Rust side is `rs/agent.rs`, and the two must stay byte-identical).

## Develop

```sh
npm install
npm test              # vitest (100% per-file coverage thresholds with `npm run test:coverage`)
npm run typecheck
npm run lint
npm run build         # bundles dist/runner.mjs with esbuild
npm run build:sandbox # build, then docker build -t vettr-sandbox -f ../sandbox/Dockerfile .
```
