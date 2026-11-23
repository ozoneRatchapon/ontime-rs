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
  owner: string;
  note: string;
  protected: boolean;
  rundowns: number;
  firstSeen: number;
  /** 0 when only registered by hand and not used since */
  lastSeen: number;
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

async function request<T>(path: string, init: RequestInit = {}, token = getAdminToken()): Promise<T> {
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

/** Exchanges the master password for the master token and keeps it for this tab. */
export async function adminLogin(password: string): Promise<void> {
  const { token } = await request<{ token: string }>(
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

/** Removes a room from the list only — its data is kept, and using it lists it again. */
export function forgetRoom(name: string): Promise<unknown> {
  return request(`/admin/rooms/${encodeURIComponent(name)}`, { method: 'DELETE' });
}

/** Screens connected to a room right now. */
export async function roomClients(name: string): Promise<RoomClient[]> {
  const { clients } = await request<{ clients: RoomClient[] }>(`/r/${encodeURIComponent(name)}/data/clients`);
  return clients;
}
