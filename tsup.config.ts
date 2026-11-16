import { defineConfig } from 'tsup';

export default defineConfig([
  {
    entry: { index: 'src/index.ts', components: 'src/components.ts', tree: 'src/tree.ts' },
    format: ['esm', 'cjs'],
    dts: true,
    clean: true,
    shims: true, // import.meta.url / __dirname in both formats (native module lookup)
    target: 'node20',
    external: ['react', 'react-dom', 'react-dom/server', /^@turbopdf\/engine-/],
    // `require('turbopdf')` returns the API itself (with .create / .default), not just { default }
    esbuildOptions: (o, { format }) => {
      o.define = { ...o.define, __TURBOPDF_CJS__: String(format === 'cjs') };
    },
    footer: ({ format }) => (format === 'cjs' ? { js: 'if (module.exports.default) module.exports = Object.assign(module.exports.default, module.exports);' } : {}),
  },
  {
    // edge runtimes and browsers: no Node API in this bundle
    entry: { edge: 'src/edge.ts' },
    format: ['esm', 'cjs'],
    dts: true,
    platform: 'neutral',
    target: 'es2022',
    external: ['react', 'react-dom', 'react-dom/server', 'markdown-it'],
  },
  {
    // the worker thread that runs the WebAssembly engine in Node, Bun and Deno
    entry: { 'wasm-worker': 'src/rust/wasm-worker.ts' },
    format: ['cjs'],
    outExtension: () => ({ js: '.cjs' }),
    platform: 'node',
    target: 'node20',
  },
  {
    entry: { cli: 'src/cli/dev.ts' },
    format: ['esm'],
    target: 'node18',
    banner: { js: '#!/usr/bin/env node' },
    esbuildOptions: (o) => {
      o.define = { ...o.define, __TURBOPDF_CJS__: 'false' };
    },
    external: ['tsx', 'react', 'react-dom', /^@turbopdf\/engine-/],
  },
]);
