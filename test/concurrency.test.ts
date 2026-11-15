import { afterEach, describe, expect, it } from 'vitest';
import turbopdf from '../src/index';
import { holdSlotUntil, withSlot } from '../src/queue';

afterEach(() => turbopdf.configure({ concurrency: 4 }));

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function maxInFlight(jobs: number): Promise<number> {
  let inflight = 0;
  let max = 0;
  await Promise.all(
    Array.from({ length: jobs }, () =>
      withSlot(undefined, async () => {
        inflight++;
        max = Math.max(max, inflight);
        await sleep(30);
        inflight--;
      }),
    ),
  );
  return max;
}

describe('concurrency', () => {
  it('never renders more PDFs at once than `concurrency`', async () => {
    turbopdf.configure({ concurrency: 2 });
    expect(await maxInFlight(6)).toBe(2);
    const pdfs = await Promise.all(Array.from({ length: 6 }, (_, i) => turbopdf.create(`<p>doc ${i}</p>`)));
    expect(pdfs.every((p) => p.pages === 1)).toBe(true);
  });

  it('with a higher limit renders in parallel', async () => {
    turbopdf.configure({ concurrency: 4 });
    expect(await maxInFlight(4)).toBe(4);
  });

  it('keeps a slot occupied while timed-out native work is still finishing', async () => {
    turbopdf.configure({ concurrency: 1 });
    let finish!: () => void;
    const completion = new Promise<void>((resolve) => { finish = resolve; });
    const visible = Promise.reject(new Error('timeout'));
    await expect(withSlot(undefined, () => holdSlotUntil(visible, completion))).rejects.toThrow('timeout');
    let started = false;
    const next = withSlot(undefined, async () => { started = true; });
    await sleep(10);
    expect(started).toBe(false);
    finish();
    await next;
    expect(started).toBe(true);
  });

  it('rejects an invalid concurrency or an unknown option', () => {
    expect(() => turbopdf.configure({ concurrency: 0 })).toThrow(/concurrency/);
    expect(() => turbopdf.configure({ nope: 1 } as never)).toThrow(/Unknown option/);
  });

  it('close() and warmup() still exist (nothing to close, warmup loads the engine)', async () => {
    await turbopdf.warmup();
    await turbopdf.close();
    expect((await turbopdf.create('<p>after close</p>')).pages).toBe(1);
  });
});

describe('abort', () => {
  it('a pre-aborted signal rejects immediately with `aborted`', async () => {
    const c = new AbortController();
    c.abort();
    await expect(turbopdf.create({ html: '<p>x</p>', signal: c.signal })).rejects.toMatchObject({ code: 'aborted' });
  });

  it('aborting while waiting in line rejects without rendering', async () => {
    turbopdf.configure({ concurrency: 1 });
    const blocker = withSlot(undefined, () => sleep(200));
    const c = new AbortController();
    const p = turbopdf.create({ html: '<p>x</p>', signal: c.signal });
    setTimeout(() => c.abort(), 20);
    await expect(p).rejects.toMatchObject({ code: 'aborted' });
    await blocker;
    expect((await turbopdf.create('<p>still works</p>')).pages).toBe(1);
  });
});
