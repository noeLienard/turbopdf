import { createElement } from 'react';
import { describe, expect, it, vi } from 'vitest';

// Simulate a project without react-dom installed.
vi.mock('react-dom/server', () => {
  throw new Error("Cannot find module 'react-dom/server'");
});

describe('react not installed', () => {
  it('throws react_not_installed and says what to install', async () => {
    const turbopdf = (await import('../src/index')).default;
    const e = await turbopdf.create(createElement('p', null, 'x')).then(() => null, (x: unknown) => x as Error & { code: string });
    expect(e?.code).toBe('react_not_installed');
    expect(e?.message).toContain('npm install react react-dom');
  });
});
