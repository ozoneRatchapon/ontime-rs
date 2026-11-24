import { getWorkerOrigin } from '../../externals';

import { AdminError } from './admin';

/**
 * A room's audit log (plan 007): who changed what. Readable by the room's
 * owners, with their session, and by the admin, with the master token.
 */

export type ActivityActor =
  { kind: 'person'; id: string; name: string } | { kind: 'roomPassword' } | { kind: 'admin' } | { kind: 'anyone' };

export type ActivityAction =
  | 'start'
  | 'load'
  | 'pause'
  | 'stop'
  | 'reload'
  | 'add-time'
  | 'roll'
  | 'entry-add'
  | 'entry-edit'
  | 'entry-delete'
  | 'entry-delete-all'
  | 'entry-move'
  | 'entry-swap'
  | 'entry-renumber'
  | 'entry-clone'
  | 'entry-group'
  | 'entry-ungroup'
  | 'delay-apply'
  | 'rundown-create'
  | 'rundown-import'
  | 'rundown-rename'
  | 'rundown-replace'
  | 'rundown-delete'
  | 'rundown-load'
  | 'rundown-duplicate'
  | 'project-data'
  | 'settings'
  | 'view-settings'
  | 'custom-fields'
  | 'url-presets'
  | 'css'
  | 'password-set'
  | 'password-cleared'
  | 'members-only-on'
  | 'members-only-off'
  | 'restore'
  | 'report-archive'
  | 'report-clear';

export interface ActivityLine {
  /** epoch ms of the latest occurrence */
  at: number;
  actor: ActivityActor;
  action: ActivityAction;
  subject?: { cue?: string; title?: string };
  /** for an edit, which fields changed; never their values */
  fields?: string[];
  /** how many more entries than the subject */
  more?: number;
  /** ms added, negative when removed */
  amount?: number;
  /** repeats merged into this line; absent means once */
  times?: number;
}

export interface ActivityPage {
  lines: ActivityLine[];
  /** pass back as `before` for the next page; null at the end */
  before: number | null;
}

/** Who is asking: an owner's session, or the admin's token. */
export type ActivityAuth = { session: string } | { token: string };

export async function roomActivity(
  room: string,
  auth: ActivityAuth,
  before: number | null = null,
): Promise<ActivityPage> {
  const query = new URLSearchParams({ limit: '100' });
  if (before !== null) query.set('before', String(before));
  const headers: Record<string, string> =
    'session' in auth ? { 'X-Ontime-Session': auth.session } : { Authorization: `Bearer ${auth.token}` };
  const response = await fetch(`${getWorkerOrigin()}/r/${encodeURIComponent(room)}/data/activity?${query}`, {
    headers,
  });
  if (!response.ok) {
    throw new AdminError(response.status, await response.text().catch(() => response.statusText));
  }
  return response.json() as Promise<ActivityPage>;
}
