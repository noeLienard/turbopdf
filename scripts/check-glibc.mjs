// Fail when a Linux (glibc) engine needs a newer glibc than MAX: it would not load on Amazon Linux 2023
// (AWS Lambda, Vercel), Debian 10 or RHEL 7, and the package would silently fall back to WebAssembly.
//
//   node scripts/check-glibc.mjs native/turbopdf.linux-x64.node
import { execFileSync } from 'node:child_process';

const MAX = [2, 17];
const file = process.argv[2];
if (!file) throw new Error('usage: node scripts/check-glibc.mjs <file.node>');
const symbols = execFileSync('objdump', ['-T', file], { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
const needs = new Map(); // version → symbols
for (const line of symbols.split('\n')) {
  const m = /\(?GLIBC_(\d+)\.(\d+)(?:\.\d+)?\)?\s+(\S+)\s*$/.exec(line);
  if (m) needs.set(`${m[1]}.${m[2]}`, [...(needs.get(`${m[1]}.${m[2]}`) ?? []), m[3]]);
}
const newer = (v) => {
  const [a, b] = v.split('.').map(Number);
  return a > MAX[0] || (a === MAX[0] && b > MAX[1]);
};
const versions = [...needs.keys()].sort((x, y) => x.split('.')[1] - y.split('.')[1]);
const max = versions.at(-1) ?? 'none';
const bad = versions.filter(newer);
console.log(`${file}: needs glibc ${max} (allowed: ${MAX.join('.')})`);
if (bad.length) {
  for (const v of bad) console.error(`  GLIBC_${v}: ${needs.get(v).slice(0, 8).join(', ')}`);
  console.error('Build it with TURBOPDF_ZIG_TARGET=<triple>.2.17 (cargo-zigbuild).');
  process.exit(1);
}
