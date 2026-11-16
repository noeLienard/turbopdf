// Release step: turn the built engines (artifacts/<target>/turbopdf.node) into one npm package per
// platform (npm/<target>/), and list them as optionalDependencies of turbopdf, at the same version.
// npm then installs only the package matching the user's OS, CPU and libc.
import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const artifacts = process.argv[2] ?? join(root, 'artifacts');
const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));

/** target → npm `os` / `cpu` / `libc` fields */
export const TARGETS = {
  'darwin-arm64': { os: 'darwin', cpu: 'arm64' },
  'darwin-x64': { os: 'darwin', cpu: 'x64' },
  'linux-x64-gnu': { os: 'linux', cpu: 'x64', libc: 'glibc' },
  'linux-arm64-gnu': { os: 'linux', cpu: 'arm64', libc: 'glibc' },
  'linux-x64-musl': { os: 'linux', cpu: 'x64', libc: 'musl' },
  'win32-x64-msvc': { os: 'win32', cpu: 'x64' },
};

const optional = {};
for (const target of readdirSync(artifacts)) {
  const meta = TARGETS[target];
  const binary = join(artifacts, target, 'turbopdf.node');
  if (!meta || !existsSync(binary)) continue;
  const dir = join(root, 'npm', target);
  mkdirSync(dir, { recursive: true });
  copyFileSync(binary, join(dir, 'turbopdf.node'));
  const name = `@turbopdf/engine-${target}`;
  writeFileSync(
    join(dir, 'package.json'),
    JSON.stringify(
      {
        name,
        version: pkg.version,
        description: `TurboPDF engine for ${target} (prebuilt; installed automatically by turbopdf)`,
        license: pkg.license,
        repository: pkg.repository,
        main: 'turbopdf.node',
        files: ['turbopdf.node'],
        os: [meta.os],
        cpu: [meta.cpu],
        ...(meta.libc ? { libc: [meta.libc] } : {}),
      },
      null,
      2,
    ) + '\n',
  );
  optional[name] = pkg.version;
  console.log(`npm/${target} → ${name}@${pkg.version}`);
}
const missing = Object.keys(TARGETS).filter((t) => !optional[`@turbopdf/engine-${t}`]);
if (missing.length) throw new Error(`missing engines: ${missing.join(', ')}`);
pkg.optionalDependencies = optional;
writeFileSync(join(root, 'package.json'), JSON.stringify(pkg, null, 2) + '\n');
