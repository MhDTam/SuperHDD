import type { Telemetry } from './types';

export type Period = 'live' | 'hour' | 'day' | 'week' | 'month';
export const windows: Record<Period, number> = {
  live: 2 * 60_000, hour: 60 * 60_000, day: 24 * 60 * 60_000,
  week: 7 * 24 * 60 * 60_000, month: 30 * 24 * 60 * 60_000,
};
export const historyKey = 'superhdd-performance-history-v1';
export interface Sample {
  start: number; end: number; seconds: number;
  hits: number; misses: number; ssdBytes: number; sourceBytes: number; evictions: number;
}
export interface History { live: Sample[]; hour: Sample[]; day: Sample[]; month: Sample[] }
const tiers = ['live', 'hour', 'day', 'month'] as const;
const resolution = { live: 0, hour: 15_000, day: 60_000, month: 30 * 60_000 };
const fields = ['seconds', 'hits', 'misses', 'ssdBytes', 'sourceBytes', 'evictions'] as const;
export const emptyHistory = (): History => ({ live: [], hour: [], day: [], month: [] });
const emptySample = (start: number, end: number): Sample => ({
  start, end, seconds: 0, hits: 0, misses: 0, ssdBytes: 0, sourceBytes: 0, evictions: 0,
});
function add(a: Sample, b: Sample): Sample {
  const result = { ...a, start: Math.min(a.start, b.start), end: Math.max(a.end, b.end) };
  for (const key of fields) result[key] += b[key];
  return result;
}
function validSample(value: unknown): value is Sample {
  if (!value || typeof value !== 'object') return false;
  const sample = value as Sample;
  return ['start', 'end', ...fields].every(key => Number.isFinite(sample[key as keyof Sample]) && sample[key as keyof Sample] >= 0)
    && sample.end > sample.start && sample.seconds > 0 && sample.seconds <= (sample.end - sample.start) / 1000 + 0.01;
}
export function loadHistory(raw: string | null, now: number): History {
  try {
    const parsed = JSON.parse(raw || 'null');
    if (parsed?.version !== 1) return emptyHistory();
    const history = emptyHistory();
    for (const tier of tiers) {
      if (!Array.isArray(parsed[tier]) || parsed[tier].length > 6000) return emptyHistory();
      history[tier] = parsed[tier].filter((s: unknown) => validSample(s) && s.end <= now && s.end > now - windows[tier])
        .sort((a: Sample, b: Sample) => a.end - b.end);
      if (history[tier].some((s, i, list) => i > 0 && s.start < list[i - 1].end)) return emptyHistory();
    }
    return history;
  } catch { return emptyHistory(); }
}
export function serializeHistory(history: History): string { return JSON.stringify({ version: 1, ...history }); }

// Reset, paused and missing intervals establish a new baseline, never a throughput spike.
export function difference(before: Telemetry, after: Telemetry, start: number, end: number): Sample | null {
  if (!before.attached || !after.attached || before.paused || after.paused || before.cacheDisabled || after.cacheDisabled
    || before.generation !== after.generation || end <= start || end - start > 10_000) return null;
  const sample: Sample = {
    start, end, seconds: (end - start) / 1000,
    hits: after.hits - before.hits, misses: after.misses - before.misses,
    ssdBytes: after.ssdReadBytes - before.ssdReadBytes, sourceBytes: after.sourceReadBytes - before.sourceReadBytes,
    evictions: after.evictions - before.evictions,
  };
  return validSample(sample) ? sample : null;
}
export function appendHistory(history: History, sample: Sample): History {
  const next = emptyHistory();
  for (const tier of tiers) {
    const items = history[tier].filter(item => item.end > sample.end - windows[tier]);
    const last = items.at(-1), size = resolution[tier];
    // A long collection gap stays visible instead of joining unrelated intervals.
    if (last && size && Math.floor(last.end / size) === Math.floor(sample.end / size) && sample.start - last.end < 2500) {
      next[tier] = [...items.slice(0, -1), add(last, sample)];
    } else next[tier] = [...items, sample];
    // Even repeated pause/resume or damaged storage cannot grow the history without bound.
    next[tier] = next[tier].slice(-6000);
  }
  return next;
}
export function selectHistory(history: History, period: Period, now: number) {
  const start = now - windows[period];
  const tier = period === 'week' ? 'month' : period;
  // Exclude a bucket crossing the lower edge rather than inventing its per-second distribution.
  const samples = history[tier].filter(item => item.start >= start && item.end <= now);
  const total = samples.reduce(add, emptySample(now, now));
  const chartSize = windows[period] / 100;
  const buckets: Sample[] = [];
  for (const sample of samples) {
    const last = buckets.at(-1);
    if (last && Math.floor(last.end / chartSize) === Math.floor(sample.end / chartSize) && sample.start - last.end < 2500) {
      buckets[buckets.length - 1] = add(last, sample);
    } else buckets.push(sample);
  }
  const points: { at: number; hit: number | null; ssd: number | null; hdd: number | null }[] = [];
  let previous: Sample | undefined;
  for (const bucket of buckets) {
    if (previous && bucket.start - previous.end > 2500) points.push({ at: previous.end + 1, hit: null, ssd: null, hdd: null });
    points.push({ at: bucket.end, hit: bucket.hits + bucket.misses ? 100 * bucket.hits / (bucket.hits + bucket.misses) : null,
      ssd: bucket.ssdBytes / bucket.seconds / 1024 ** 3, hdd: bucket.sourceBytes / bucket.seconds / 1024 ** 3 });
    previous = bucket;
  }
  return { start, end: now, samples, total, points };
}

// Only used by the explicitly labelled UI demo; never merged with collected history.
export function demoHistory(now: number): History {
  const result = emptyHistory();
  for (const tier of tiers) {
    const step = resolution[tier] || 1500;
    for (let end = now - windows[tier] + step; end <= now; end += step) {
      const hit = 45 + Math.round(25 * (1 + Math.sin(end / 60_000)) / 2);
      result[tier].push({ start: end - step, end, seconds: step / 1000, hits: hit, misses: 100 - hit,
        ssdBytes: hit * step * 1024, sourceBytes: (100 - hit) * step * 1024, evictions: 1 });
    }
  }
  return result;
}
