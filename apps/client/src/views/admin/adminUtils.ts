import type { RoomClient } from '../../common/api/admin';

const relative = new Intl.RelativeTimeFormat(undefined, { numeric: 'auto' });

const steps: [Intl.RelativeTimeFormatUnit, number][] = [
  ['day', 86_400_000],
  ['hour', 3_600_000],
  ['minute', 60_000],
];

/** "5 minutes ago", or "never" for a room only registered by hand. */
export function formatLastSeen(timestamp: number, now = Date.now()): string {
  if (!timestamp) return 'never';
  const elapsed = timestamp - now;
  for (const [unit, size] of steps) {
    if (Math.abs(elapsed) >= size) {
      return relative.format(Math.round(elapsed / size), unit);
    }
  }
  return 'just now';
}

export function formatDate(timestamp: number): string {
  return timestamp ? new Date(timestamp).toLocaleString() : '';
}

/** "timer ×2, editor" — what the connected screens are showing. */
export function summariseClients(clients: RoomClient[]): string {
  const counts = new Map<string, number>();
  for (const client of clients) {
    const view = client.path?.replace(/^\//, '').split(/[/?]/)[0] || 'unknown';
    counts.set(view, (counts.get(view) ?? 0) + 1);
  }
  return [...counts.entries()].map(([view, count]) => (count > 1 ? `${view} ×${count}` : view)).join(', ');
}

/** A link into a room's view on this client. */
export function roomLink(room: string, view: string): string {
  const url = new URL(`/${view}`, window.location.origin);
  url.searchParams.set('room', room);
  return url.toString();
}
