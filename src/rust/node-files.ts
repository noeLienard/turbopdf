// Files for the WebAssembly engine in Node, Bun and Deno: inside baseDir only, with the native module's
// rules and messages (no `..` escape, symbolic links resolved and checked).
import { readFileSync, realpathSync } from 'node:fs';
import { isAbsolute, normalize, resolve, sep } from 'node:path';
import type { Files } from './wasm-core';

export const nodeFiles: Files = {
  base: (dir) => realpathSync(dir),
  read(base, path) {
    if (!isAbsolute(path)) {
      let depth = 0;
      for (const part of normalize(path).split(/[\\/]/)) {
        if (part === '..') depth--;
        else if (part && part !== '.') depth++;
        if (depth < 0) throw new Error(`${JSON.stringify(path)} is outside baseDir (${base})`);
      }
    }
    let real: string;
    try {
      real = realpathSync(resolve(base, path));
    } catch {
      throw new Error(`${JSON.stringify(path)} was not found in baseDir (${base})`);
    }
    // symbolic links are followed: the file itself must be inside too
    if (real !== base && !real.startsWith(base.endsWith(sep) ? base : base + sep)) {
      throw new Error(`${JSON.stringify(path)} is outside baseDir (${base})`);
    }
    try {
      return readFileSync(real);
    } catch (e) {
      throw new Error(`cannot read ${JSON.stringify(path)}: ${(e as Error).message}`);
    }
  },
};
