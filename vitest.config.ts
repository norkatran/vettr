import { resolve } from 'node:path'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  resolve: { alias: { '@shared': resolve(__dirname, 'src/shared') } },
  test: {
    include: ['src/**/*.test.ts'],
    coverage: {
      provider: 'v8',
      // Listing src explicitly makes untested files count as 0% instead of being invisible
      include: ['src/**/*.{ts,tsx}'],
      exclude: [
        'src/**/*.test.ts',
        'src/**/*.d.ts',
        // Electron and React glue with no extractable logic; keep it thin
        'src/main/index.ts',
        'src/runner/index.ts',
        'src/preload/index.ts',
        'src/renderer/src/**'
      ],
      reporter: ['text', 'lcov'],
      thresholds: { perFile: true, lines: 100, functions: 100, branches: 100, statements: 100 }
    }
  }
})
