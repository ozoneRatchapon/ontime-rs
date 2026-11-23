import { AdminError, Person, RoomRecord, workerRequest } from './admin';

/**
 * A person's own sign-in on this device, made by opening an invite link.
 *
 * Kept in localStorage rather than sessionStorage: remembering the device is
 * the point, so the link only has to be opened once. In this phase it opens no
 * room — room passwords work as before.
 */

const SESSION_KEY = 'ontime.session';

export function getSession(): string | null {
  try {
    return localStorage.getItem(SESSION_KEY);
  } catch {
    return null;
  }
}

function setSession(token: string | null) {
  try {
    if (token) localStorage.setItem(SESSION_KEY, token);
    else localStorage.removeItem(SESSION_KEY);
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
export async function whoAmI(): Promise<{ person: Person; rooms: RoomRecord[] } | null> {
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
