import { defineConfig } from 'vitest/config'
import solid from 'vite-plugin-solid'

/**
 * Separate from `vite.config.ts` so the app build is unaffected by test-only
 * settings. `conditions: ['development', 'browser']` is required for Solid:
 * without it the server build is resolved and reactivity does not run.
 */
export default defineConfig({
  plugins: [solid()],
  resolve: { conditions: ['development', 'browser'] },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./src/test/setup.ts'],
    include: ['src/**/*.test.{ts,tsx}'],
  },
})
