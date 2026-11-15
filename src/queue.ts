import { PapierError } from './errors';
import { loadNative } from './rust/native';
import type { ConfigureOptions } from './types';

/**
 * At most `concurrency` renders run at once; the others wait in line. Each render uses a thread of the
 * libuv pool, so this also keeps room for your own file and crypto work.
 */
let concurrency = 4;
let running = 0;
const queue: Array<() => void> = [];
const slotCompletions = new WeakMap<Promise<unknown>, Promise<unknown>>();

/** Keep a concurrency slot until background native work settles, even if its public timeout/abort rejects first. */
export function holdSlotUntil<T>(visible: Promise<T>, completion: Promise<unknown>): Promise<T> {
  slotCompletions.set(visible, completion);
  return visible;
}

export function configure(options: ConfigureOptions): void {
  const known = new Set(['concurrency', 'keepAlive']);
  for (const key of Object.keys(options)) if (!known.has(key)) throw new PapierError('invalid_source', `Unknown option "${key}" for configure(). Use: concurrency.`);
  if (options.concurrency !== undefined) {
    if (!Number.isInteger(options.concurrency) || options.concurrency < 1) throw new PapierError('invalid_source', '`concurrency` must be an integer >= 1.');
    concurrency = options.concurrency;
    drain();
  }
  // `keepAlive` configured the browser of earlier versions: accepted and ignored
}

/** Hand free slots to waiting renders: the slot is taken here, before the waiter resumes. */
function drain(): void {
  while (running < concurrency && queue.length) {
    running++;
    queue.shift()!();
  }
}

/** Run `work` when a slot is free. Aborting while waiting leaves the line without rendering. */
export async function withSlot<T>(signal: AbortSignal | undefined, work: () => Promise<T>): Promise<T> {
  if (running >= concurrency) {
    await new Promise<void>((resolve, reject) => {
      const start = () => {
        signal?.removeEventListener('abort', onAbort);
        resolve();
      };
      const onAbort = () => {
        const i = queue.indexOf(start);
        if (i >= 0) queue.splice(i, 1);
        reject(new PapierError('aborted', 'Rendering was aborted while waiting for a free slot.'));
      };
      signal?.addEventListener('abort', onAbort, { once: true });
      queue.push(start);
    });
  } else {
    running++;
  }
  let deferredRelease = false;
  let released = false;
  const release = () => {
    if (released) return;
    released = true;
    running--;
    drain();
  };
  try {
    const visible = work();
    const completion = slotCompletions.get(visible);
    if (completion) {
      deferredRelease = true;
      void completion.then(release, release);
    }
    return await visible;
  } finally {
    if (!deferredRelease) release();
  }
}

/** Load the engine now (module + embedded fonts) so the first PDF does not pay for it. */
export async function warmup(): Promise<void> {
  await loadNative().render(JSON.stringify({ type: 'document', version: 1, children: [{ type: 'page', children: [{ type: 'text', content: 'a' }] }] }));
}

/** Nothing to close: there is no browser. Kept so code written for earlier versions keeps working. */
export async function close(): Promise<void> {}
