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
 * Sets or clears this room's password. Passing null reopens the room.
 *
 * When a password is set the response carries the bearer token once — it is the
 * hash of the password and cannot be recovered later, so the caller must keep it
 * (we store it so the current session stays authorised).
 */
export async function setRoomPassword(password: string | null): Promise<{ protected: boolean; token?: string }> {
  const res = await axios.post(`${sessionPath}/password`, { password });
  return res.data;
}
