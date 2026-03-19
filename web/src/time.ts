export function relativeTime(dateStr: string, now: number = Date.now()): string {
  const then = new Date(dateStr).getTime();
  const diff = Math.max(0, now - then);
  const sec = Math.floor(diff / 1000);

  if (sec < 10) return 'just now';
  if (sec < 60) return `${sec}s ago`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h ago`;
  const days = Math.floor(hr / 24);
  if (days < 30) return `${days}d ago`;
  return new Date(dateStr).toLocaleDateString();
}
