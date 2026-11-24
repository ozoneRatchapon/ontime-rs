import path from 'path';

import { sentryVitePlugin } from '@sentry/vite-plugin';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';
import svgrPlugin from 'vite-plugin-svgr';

import { ONTIME_VERSION } from './src/ONTIME_VERSION.js';

const sentryAuthToken = process.env.SENTRY_AUTH_TOKEN;

const compiler = {
  compilationMode: 'annotation',
};

// Upstream's vite 8 config, with three local choices:
// - the legacy build stays off, as it was before this upgrade
// - no precompressed .br files: Cloudflare Pages compresses on its own
// - the alias for the Rust timer engine, below
export default defineConfig({
  base: './', // Ontime cloud: we use relative paths to allow them to reference a dynamic base set at runtime
  define: {
    // we pass along the NODE_ENV here in case it is a docker build
    'import.meta.env.IS_DOCKER': process.env.NODE_ENV === 'docker',
  },
  plugins: [
    react({ compiler }),
    svgrPlugin(),
    sentryAuthToken &&
      sentryVitePlugin({
        org: 'get-ontime',
        project: 'ontime',
        include: './build',
        authToken: sentryAuthToken,
        release: ONTIME_VERSION,
        deploy: {
          env: 'production',
        },
        bundleSizeOptimizations: {
          excludeDebugStatements: true,
          excludeReplayIframe: true,
          excludeReplayShadowDom: true,
          excludeReplayWorker: true,
        },
      }),
  ].filter(Boolean),
  server: {
    port: 3000,
    proxy: {
      '^/login*': {
        target: 'http://localhost:4001/',
        changeOrigin: true,
      },
      '^/data*': {
        target: 'http://localhost:4001/',
        changeOrigin: true,
      },
      '^/api*': {
        target: 'http://localhost:4001/',
        changeOrigin: true,
      },
      '^/mcp': {
        target: 'http://localhost:4001/',
        changeOrigin: true,
      },
      '^/ws*': {
        target: 'http://localhost:4001/',
        changeOrigin: true,
        ws: true,
      },
      '^/user*': {
        target: 'http://localhost:4001/',
        changeOrigin: true,
        ws: true,
      },
    },
  },
  test: {
    globals: true,
    dom: true,
  },
  build: {
    outDir: './build',
    sourcemap: true,
  },
  resolve: {
    alias: {
      '@': path.resolve(import.meta.dirname, './src'),
      // Rust timer engine compiled to WebAssembly. Built by
      // packages/ontime-wasm/build.sh; aliased rather than installed so the
      // vendored client keeps its own dependency list untouched.
      'ontime-timer-engine': path.resolve(import.meta.dirname, '../../packages/ontime-wasm/pkg/ontime_timer.js'),
    },
  },
  css: {
    preprocessorOptions: {
      scss: {
        additionalData: `
        @use '@/theme/themeTokens' as *;
        `,
      },
    },
  },
});
