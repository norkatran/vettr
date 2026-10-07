import { defineConfig } from 'vitest/config'

export default defineConfig({
  test: {
    include: ['src/**/*.test.ts'],
    coverage: {
      provider: 'v8',
      // Listing src explicitly makes untested files count as 0% instead of being invisible
      include: ['src/**/*.ts'],
      // The entry point is stdio and SDK glue with no extractable logic; keep it thin
      exclude: ['src/**/*.test.ts', 'src/index.ts'],
      reporter: ['text', 'lcov'],
      thresholds: { perFile: true, lines: 100, functions: 100, branches: 100, statements: 100 }
    }
  }
})
