import path from 'path';

import { sentryVitePlugin } from '@sentry/vite-plugin';
import legacy from '@vitejs/plugin-legacy';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';
import { compression } from 'vite-plugin-compression2';
import svgrPlugin from 'vite-plugin-svgr';

import { ONTIME_VERSION } from './src/ONTIME_VERSION';

const sentryAuthToken = process.env.SENTRY_AUTH_TOKEN;

const ReactCompilerConfig = {
  compilationMode: 'annotation',
};

export default defineConfig({
  base: './', // Ontime cloud: we use relative paths to allow them to reference a dynamic base set at runtime
  define: {
    // we pass along the NODE_ENV here in case it is a docker build
    'import.meta.env.IS_DOCKER': process.env.NODE_ENV === 'docker',
  },
  plugins: [
    // legacy({
    //   targets: ['safari >= 14.1'],
    // }),
    react({
      babel: {
        plugins: [['babel-plugin-react-compiler', ReactCompilerConfig]],
      },
    }),
    svgrPlugin(),
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
    onLog: () => {},
  },
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
      // Rust timer engine compiled to WebAssembly. Built by
      // packages/ontime-wasm/build.sh; aliased rather than installed so the
      // vendored client keeps its own dependency list untouched.
      'ontime-timer-engine': path.resolve(__dirname, '../../packages/ontime-wasm/pkg/ontime_timer.js'),
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
