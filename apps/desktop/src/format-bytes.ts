/** Decimal units match the cache capacity input and disk manufacturers. */
export function formatBytes(value: number): string {
  const bytes = Number.isFinite(value) ? Math.max(0, value) : 0;
  const units = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];
  let unit = Math.min(Math.floor(Math.log10(bytes || 1) / 3), units.length - 1);
  unit = Math.max(0, unit);
  const amount = bytes / 1000 ** unit;
  const digits = unit === 0 ? 0 : unit >= 3 || amount < 10 ? 2 : 1;
  // Promote values that would round to 1000 in the smaller unit.
  if (Number(amount.toFixed(digits)) >= 1000 && unit < units.length - 1) {
    return `${(amount / 1000).toFixed(2)} ${units[unit + 1]}`;
  }
  return `${amount.toFixed(digits)} ${units[unit]}`;
}
