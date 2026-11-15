# Contributing to Papier

Thanks for helping. Papier is small on purpose: **create, download, preview**. Features outside that scope (templates, a cloud API, webhooks) are out of scope.

## Setup

```bash
git clone <repo> && cd papier
npm install
npm run build:native      # builds the Rust engine into native/ (needs Rust: https://rustup.rs)
```

Requires Node.js 20+ and a Rust toolchain (edition 2024).

## Everyday commands

```bash
npm test               # all SDK tests
cargo test --manifest-path engine/Cargo.toml   # engine tests
npx vitest run test/pagebreak.test.ts   # one file
npm run typecheck      # tsc --noEmit
npm run build          # tsup → dist/ (ESM + CJS + types + CLI)
npm run check          # all TypeScript, Node and Rust quality gates
npx tsx src/cli/dev.ts dev examples/rapport.md   # try the live preview from source
```

## Project layout

```
src/index.ts       public API: default export + named exports
src/create.ts      option validation, the Pdf object
src/queue.ts       concurrency limit, warmup()
src/preview.ts     PNG pages from the engine
src/components.ts  papier/components (React → document tree)
src/tree.ts        papier/tree types (generated from engine/crates/papier-tree)
src/rust/engine.ts source → document tree → native render, warnings
src/rust/markup.ts HTML / React markup → document tree
src/rust/cascade.ts CSS selectors, specificity, @media, @page
src/rust/css.ts    CSS declarations → tree styles
src/rust/markdown.ts Markdown → document tree
src/rust/native.ts loads native/papier.<platform>-<arch>.node
src/cli/           `papier dev`
engine/            the Rust engine (see engine/README.md)
test/              one file per theme (options, fonts, pagebreak, warnings…)
```

## Conventions

- **TypeScript strict.** Every public option is typed and documented with a JSDoc comment (it shows in the editor).
- **Errors are `PapierError`** with a `code` and a message that says *what to do*, not only what went wrong.
- **Warnings are returned, never thrown.** A render should still give a PDF.
- **Every option has a test.** Prove behavior on the PDF itself (page count, text per page, pixels), not on internal state. Helpers are in `test/helpers.ts`.
- A bug fix starts with a failing test.
- Tests that need a server use a local one (`serve()` in the helpers); never rely on the internet.
- Keep dependencies few. React stays an optional peer dependency.
- Commit messages: short, imperative (`Fix footer margin when only header is set`).

## Things that are easy to get wrong

- Functions passed to `page.evaluate` are serialized: they cannot use variables from the outer scope.
- `page.pdf()` header/footer templates are isolated from the page: no CSS inheritance.
- HTML served in tests needs `charset=utf-8`, or accents and symbols are decoded as Latin-1.
- Do not run `npm install <package>` casually: npm sometimes drops the native Rolldown binary that Vitest needs. If `vitest` says "Cannot find native binding", delete `node_modules` and `package-lock.json` and run `npm install` again.

## Pull requests

1. Open an issue first for anything that changes the public API.
2. Add or update tests, and a line in `CHANGELOG.md` under *Unreleased*.
3. `npm run check` must pass.
