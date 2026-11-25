import axios from 'axios';
import { GetInfo, LinkOptions } from 'ontime-types';

import { apiEntryUrl } from './constants';
import type { RequestOptions } from './requestOptions';

const sessionPath = `${apiEntryUrl}/session`;

/**
 * HTTP request to retrieve application info
 */
export async function getInfo(options?: RequestOptions): Promise<GetInfo> {
  const res = await axios.get(`${sessionPath}/info`, { signal: options?.signal });
  return res.data;
}

/**
 * HTTP request to get a pre-authenticated URL
 */
export async function generateUrl(options: LinkOptions & { baseUrl: string; path: string }): Promise<string> {
  const res = await axios.post(`${sessionPath}/url`, options);
  return res.data.url;
}

/**
 * Locks this room with a new random token, and returns it. Locking an already
 * locked room replaces its token, so every link shared before stops working.
 * The token is not made from a password, so it cannot be guessed (plan 019).
 */
export async function lockRoom(): Promise<{ protected: boolean; token: string }> {
  const res = await axios.post(`${sessionPath}/password`, { lock: true });
  return res.data;
}

/** Removes the lock: anyone with the room's address can view and edit it again. */
export async function reopenRoom(): Promise<{ protected: boolean }> {
  const res = await axios.post(`${sessionPath}/password`, { password: null });
  return res.data;
}
