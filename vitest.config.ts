import { defineConfig } from 'vitest/config';
import path from 'path';

export default defineConfig({
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  test: {
    // Core layout/format logic is pure — no DOM needed.
    environment: 'node',
    include: ['src/**/*.test.ts'],
  },
});
