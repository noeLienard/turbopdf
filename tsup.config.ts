import { defineConfig } from 'tsup';

export default defineConfig([
  {
    entry: { index: 'src/index.ts', components: 'src/components.ts', tree: 'src/tree.ts' },
    format: ['esm', 'cjs'],
    dts: true,
    clean: true,
    shims: true, // import.meta.url / __dirname in both formats (native module lookup)
    target: 'node20',
    external: ['react', 'react-dom', 'react-dom/server'],
    // `require('turbopdf')` returns the API itself (with .create / .default), not just { default }
    footer: ({ format }) => (format === 'cjs' ? { js: 'if (module.exports.default) module.exports = Object.assign(module.exports.default, module.exports);' } : {}),
  },
  {
    entry: { cli: 'src/cli/dev.ts' },
    format: ['esm'],
    target: 'node18',
    banner: { js: '#!/usr/bin/env node' },
    external: ['tsx', 'react', 'react-dom'],
  },
]);
