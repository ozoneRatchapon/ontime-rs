import { getWorkerOrigin } from '../../externals';

/**
 * The operator's view across rooms, behind the Worker's master password.
 *
 * Plain fetch rather than the shared axios instance: that one attaches the
 * current room's token, and these calls need the master token instead.
 */

export interface RoomRecord {
  name: string;
  title: string;
  /** free-text label from before people existed; shown until cleared */
  owner: string;
  /** ids of the people who own the room */
  owners: string[];
  /** ids of the people who may run the room */
  operators: string[];
  /** only members (or the room/master password) may edit or drive it */
  membersOnly: boolean;
  note: string;
  protected: boolean;
  rundowns: number;
  firstSeen: number;
  /** 0 when only registered by hand and not used since */
  lastSeen: number;
}

export interface Person {
  id: string;
  name: string;
  createdAt: number;
  /** the owner who invited them; null when the admin did */
  invitedBy: string | null;
}

export type Role = 'owner' | 'operator';

/** A room as one of its members sees it. */
export interface MemberRoom extends RoomRecord {
  role: Role;
}

export interface PersonSummary extends Person {
  /** one label per signed-in device, e.g. "iPhone Safari" */
  devices: string[];
  lastSeen: number;
  /** set while an invite link is waiting to be opened */
  inviteExpiresAt: number | null;
}

export interface RoomClient {
  name: string;
  type: string;
  path: string;
}

const TOKEN_KEY = 'ontime.admin.token';

export function getAdminToken(): string | null {
  try {
    return sessionStorage.getItem(TOKEN_KEY);
  } catch {
    return null;
  }
}

export function clearAdminToken() {
  try {
    sessionStorage.removeItem(TOKEN_KEY);
  } catch {
    // nothing stored to clear
  }
}

export class AdminError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message);
  }
}

/** A JSON call to the Worker with the given bearer token. */
export async function workerRequest<T>(path: string, init: RequestInit = {}, token: string | null): Promise<T> {
  const response = await fetch(`${getWorkerOrigin()}${path}`, {
    ...init,
    headers: {
      'content-type': 'application/json',
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
  });
  if (!response.ok) {
    const message = await response.text().catch(() => '');
    throw new AdminError(response.status, message || response.statusText);
  }
  return response.json() as Promise<T>;
}

function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  return workerRequest<T>(path, init, getAdminToken());
}

/** Exchanges the master password for the master token and keeps it for this tab. */
export async function adminLogin(password: string): Promise<void> {
  const { token } = await workerRequest<{ token: string }>(
    '/admin/login',
    { method: 'POST', body: JSON.stringify({ password }) },
    null,
  );
  try {
    sessionStorage.setItem(TOKEN_KEY, token);
  } catch {
    // private browsing: the page works until reload
  }
}

export async function listRooms(): Promise<RoomRecord[]> {
  const { rooms } = await request<{ rooms: RoomRecord[] }>('/admin/rooms');
  return rooms;
}

/** Sets owner and/or note; registers the room if the list does not know it yet. */
export function saveRoomLabels(name: string, labels: { owner?: string; note?: string }): Promise<RoomRecord> {
  return request<RoomRecord>(`/admin/rooms/${encodeURIComponent(name)}`, {
    method: 'PUT',
    body: JSON.stringify(labels),
  });
}

/** Replaces who owns, or who runs, a room. */
export function saveRoomMembers(
  name: string,
  members: { owners?: string[]; operators?: string[] },
): Promise<RoomRecord> {
  return request<RoomRecord>(`/admin/rooms/${encodeURIComponent(name)}`, {
    method: 'PUT',
    body: JSON.stringify(members),
  });
}

export async function listPeople(): Promise<PersonSummary[]> {
  const { people } = await request<{ people: PersonSummary[] }>('/admin/people');
  return people;
}

export function addPerson(name: string): Promise<Person> {
  return request<Person>('/admin/people', { method: 'POST', body: JSON.stringify({ name }) });
}

/** A fresh invite link for a person; any unused earlier link stops working. */
export async function createInvite(id: string): Promise<{ link: string; expiresAt: number }> {
  const { code, expiresAt } = await request<{ code: string; expiresAt: number }>(
    `/admin/people/${encodeURIComponent(id)}/invite`,
    { method: 'POST' },
  );
  return { link: inviteLink(code), expiresAt };
}

/** The code rides in the fragment, which never reaches a server log. */
export function inviteLink(code: string): string {
  return `${window.location.origin}/join#code=${code}`;
}

/** Signs a person out of every device and cancels their invite link. */
export function signOutPerson(id: string): Promise<unknown> {
  return request(`/admin/people/${encodeURIComponent(id)}/sessions`, { method: 'DELETE' });
}

/** Removes a person, and takes them off every room they own. */
export function removePerson(id: string): Promise<unknown> {
  return request(`/admin/people/${encodeURIComponent(id)}`, { method: 'DELETE' });
}

/** Removes a room from the list only — its data is kept, and using it lists it again. */
export function forgetRoom(name: string): Promise<unknown> {
  return request(`/admin/rooms/${encodeURIComponent(name)}`, { method: 'DELETE' });
}

/** Screens connected to a room right now. */
export async function roomClients(name: string): Promise<RoomClient[]> {
  const { clients } = await request<{ clients: RoomClient[] }>(`/r/${encodeURIComponent(name)}/data/clients`);
  return clients;
}
