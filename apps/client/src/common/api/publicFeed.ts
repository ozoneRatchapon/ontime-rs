import axios from 'axios';

import { apiEntryUrl } from './constants';

/** What a room shows its audience without a token (plan 023, Worker only). */
export type PublicFeed = { enabled: boolean; fields: string[] };

const path = `${apiEntryUrl}/public-feed`;

export async function getPublicFeed(): Promise<PublicFeed> {
  return (await axios.get<PublicFeed>(path)).data;
}

export async function setPublicFeed(change: Partial<PublicFeed>): Promise<PublicFeed> {
  return (await axios.post<PublicFeed>(path, change)).data;
}
