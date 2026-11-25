import { getWorkerOrigin } from '../../externals';
import { AdminError } from './admin';

/**
 * A room's audit log (plan 007): who changed what. Readable by the room's
 * owners, with their session, and by the admin, with the master token.
 */

export type ActivityActor =
  | { kind: 'person'; id: string; name: string }
  | { kind: 'roomPassword' }
  | { kind: 'admin' }
  | { kind: 'anyone' };

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
  | 'project-import'
  | 'custom-view-add'
  | 'custom-view-remove'
  | 'report-archive'
  | 'report-clear'
  // people events, on the admin page
  | 'person-create'
  | 'person-remove'
  | 'invite-issue'
  | 'invite-redeem'
  | 'sign-out'
  | 'members-change'
  | 'operator-add'
  | 'operator-remove'
  // the shared teleprompter
  | 'prompter-play'
  | 'prompter-pause'
  | 'prompter-move'
  | 'prompter-speed'
  | 'prompter-mode'
  | 'prompter-blackout';

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
  /** the room a people event is about */
  room?: string;
}

export interface ActivityPage {
  lines: ActivityLine[];
  /** pass back as `before` for the next page; null at the end */
  before: number | null;
}

/** Who is asking: an owner's session, or the admin's token. */
export type ActivityAuth = { session: string } | { token: string };

/** One page of a room's log. */
export function roomActivity(room: string, auth: ActivityAuth, before: number | null = null): Promise<ActivityPage> {
  return activityPage(`/r/${encodeURIComponent(room)}/data/activity`, auth, before);
}

/** One page of the people events, for the admin. */
export function peopleActivity(token: string, before: number | null = null): Promise<ActivityPage> {
  return activityPage('/admin/activity', { token }, before);
}

async function activityPage(path: string, auth: ActivityAuth, before: number | null): Promise<ActivityPage> {
  const query = new URLSearchParams({ limit: '100' });
  if (before !== null) query.set('before', String(before));
  const headers: Record<string, string> =
    'session' in auth ? { 'X-Ontime-Session': auth.session } : { Authorization: `Bearer ${auth.token}` };
  const response = await fetch(`${getWorkerOrigin()}${path}?${query}`, { headers });
  if (!response.ok) {
    throw new AdminError(response.status, await response.text().catch(() => response.statusText));
  }
  return response.json() as Promise<ActivityPage>;
}
