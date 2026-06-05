import { defineConfig } from 'tsup';

export default defineConfig([
  // Main ESM + CJS + .d.ts
  {
    entry: { index: 'src/index.ts', node: 'src/node.ts' },
    format: ['esm', 'cjs'],
    dts: true,
    splitting: false,
    sourcemap: true,
    clean: true,
    target: 'es2020',
    platform: 'neutral',
    esbuildOptions(options) {
      options.conditions = ['import', 'default'];
    },
  },
  // Browser UMD bundle
  {
    entry: { 'browser.min': 'src/browser.ts' },
    format: ['iife'],
    globalName: 'RustChat',
    dts: false,
    splitting: false,
    sourcemap: true,
    minify: true,
    target: 'es2020',
    platform: 'browser',
    define: { 'process.env.NODE_ENV': '"production"' },
  },
]);
