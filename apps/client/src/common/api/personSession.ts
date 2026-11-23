import { PERSON_SESSION_KEY, getPersonSession, getWorkerOrigin } from '../../externals';

import { AdminError, MemberRoom, Person, PersonSummary, inviteLink, workerRequest } from './admin';

/**
 * A person's own sign-in on this device, made by opening an invite link.
 *
 * Kept in localStorage rather than sessionStorage: remembering the device is
 * the point, so the link only has to be opened once. In this phase it opens no
 * room — room passwords work as before.
 */

export const getSession = getPersonSession;

function setSession(token: string | null) {
  try {
    if (token) localStorage.setItem(PERSON_SESSION_KEY, token);
    else localStorage.removeItem(PERSON_SESSION_KEY);
  } catch {
    // private browsing: signed in until the tab closes
  }
}

/** Spends an invite code and remembers this device. */
export async function redeemInvite(code: string): Promise<Person> {
  const { token, person } = await workerRequest<{ token: string; person: Person }>(
    '/auth/redeem',
    { method: 'POST', body: JSON.stringify({ code }) },
    null,
  );
  setSession(token);
  return person;
}

/** Who this device is signed in as, and the rooms they own. Null when signed out. */
export async function whoAmI(): Promise<{ person: Person; rooms: MemberRoom[] } | null> {
  const token = getSession();
  if (!token) return null;
  try {
    return await workerRequest('/auth/me', {}, token);
  } catch (err) {
    if (err instanceof AdminError && err.status === 401) {
      setSession(null);
      return null;
    }
    throw err;
  }
}

export async function signOut(): Promise<void> {
  const token = getSession();
  setSession(null);
  if (token) {
    await workerRequest('/auth/logout', { method: 'POST' }, token).catch(() => undefined);
  }
}

// #region an owner's team ===========================================

export interface TeamMember {
  person: PersonSummary;
  /** invited by the caller, so they may make a new link for them */
  mine: boolean;
}

function asMe<T>(path: string, init: RequestInit = {}): Promise<T> {
  return workerRequest<T>(path, init, getSession());
}

const teamPath = (room: string) => `/auth/rooms/${encodeURIComponent(room)}`;

export function getTeam(room: string): Promise<{ owners: TeamMember[]; operators: TeamMember[] }> {
  return asMe(`${teamPath(room)}/team`);
}

/** Makes a new operator for the room and their first invite link. */
export async function addOperator(room: string, name: string): Promise<{ person: Person; link: string; expiresAt: number }> {
  const made = await asMe<{ person: Person; code: string; expiresAt: number }>(`${teamPath(room)}/operators`, {
    method: 'POST',
    body: JSON.stringify({ name }),
  });
  return { person: made.person, link: inviteLink(made.code), expiresAt: made.expiresAt };
}

/** A new link for an operator this owner invited; an unused earlier one stops working. */
export async function relinkOperator(room: string, id: string): Promise<{ link: string; expiresAt: number }> {
  const made = await asMe<{ code: string; expiresAt: number }>(
    `${teamPath(room)}/operators/${encodeURIComponent(id)}/invite`,
    { method: 'POST' },
  );
  return { link: inviteLink(made.code), expiresAt: made.expiresAt };
}

export function removeOperator(room: string, id: string): Promise<unknown> {
  return asMe(`${teamPath(room)}/operators/${encodeURIComponent(id)}`, { method: 'DELETE' });
}

/** Turns "editing needs sign-in" on or off — the room itself checks this is an owner. */
export async function setMembersOnly(room: string, on: boolean): Promise<void> {
  const session = getSession();
  const response = await fetch(`${getWorkerOrigin()}/r/${encodeURIComponent(room)}/data/members-only`, {
    method: 'POST',
    headers: { 'content-type': 'application/json', ...(session ? { 'X-Ontime-Session': session } : {}) },
    body: JSON.stringify({ on }),
  });
  if (!response.ok) {
    throw new AdminError(response.status, await response.text().catch(() => response.statusText));
  }
}

// #endregion
