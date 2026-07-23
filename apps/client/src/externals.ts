/**
 * This file contains a list of constants that may need to be resolved at runtime
 */

import { version } from '../../../package.json';

export const githubUrl = 'https://www.github.com/cpvalente/ontime';
export const apiRepoLatest = 'https://api.github.com/repos/cpvalente/ontime/releases/latest';
export const websiteUrl = 'https://www.getontime.no';
export const discordUrl = 'https://discord.com/invite/eje3CSUEXm';
export const subredditUrl = 'https://www.reddit.com/r/ontimeapp/';
export const youtubeUrl = 'https://www.youtube.com/@ontimeapp';

export const documentationUrl = 'https://docs.getontime.no';
export const customFieldsDocsUrl = 'https://docs.getontime.no/features/custom-fields/';

export const githubSponsorUrl = 'https://github.com/sponsors/cpvalente';
export const buyMeACoffeeUrl = 'https://buymeacoffee.com/cpvalente';

// resolve environment
export const appVersion = version;
export const isDocker = import.meta.env.IS_DOCKER; // this env is made available by the vite.config.js define function
export const isProduction = import.meta.env.PROD;
export const isDev = import.meta.env.DEV;
export const currentHostName = window.location.hostname;
export const isLocalhost = currentHostName === 'localhost' || currentHostName === '127.0.0.1';
export const isOntimeCloud = document.querySelector('base')?.hasAttribute('data-is-cloud');

export const isTouchDevice = 'ontouchstart' in window || navigator.maxTouchPoints > 0;
export const supportsFullscreen = document.fullscreenEnabled;

// resolve entrypoint URLs

/**
 * Resolve base
 * @example '' for electron
 * @example '/client-hash' for cloud
 */
export const baseURI = resolveBaseURI();

/**
 * The room this session booted with, captured once.
 *
 * Read at module load rather than on demand: navigation can drop `?room=` from the
 * address bar, and everything downstream still has to keep talking to the same
 * project. `useStickyRoom` puts it back in the URL.
 */
export const activeRoom = getRoom();
export const serverURL = getBaseServerUrl();

/**
 * Where the *client* is served from.
 *
 * Upstream this is the same host as the API, so the two are interchangeable. Here
 * the client is on Pages and the backend on Workers, so a link to a view has to be
 * built from this and never from `serverURL` — the worker serves no HTML.
 */
export const clientURL = window.location.origin;
export const websocketUrl = resolveUrl('ws', 'ws');

/**
 * Reads `?room=` and validates it the same way the worker does.
 * A room is one isolated project; without one the shared default room is used.
 */
export function getRoom(): string | null {
  const room = new URLSearchParams(window.location.search).get('room');
  if (!room || room.length > 64 || !/^[A-Za-z0-9_-]+$/.test(room)) {
    return null;
  }
  return room;
}

/**
 * The bearer token for a protected room.
 *
 * Read from `?token=` once and kept in sessionStorage keyed by room, so the token
 * does not have to stay in the address bar (where it would leak through screen
 * shares and browser history) and so each room keeps its own.
 */
export function getToken(): string | null {
  const room = activeRoom ?? getRoom() ?? 'default';
  const storageKey = `ontime.token.${room}`;

  const fromUrl = new URLSearchParams(window.location.search).get('token');
  if (fromUrl) {
    try {
      sessionStorage.setItem(storageKey, fromUrl);
    } catch {
      // private browsing can refuse storage; the token still works for this load
    }
    return fromUrl;
  }

  try {
    return sessionStorage.getItem(storageKey);
  } catch {
    return null;
  }
}

function getBaseServerUrl() {
  const customWorkerUrl = import.meta.env.VITE_WORKER_URL;
  const origin = customWorkerUrl ? customWorkerUrl.replace(/\/+$/, '') : window.location.origin;

  // REST paths are composed by appending to this URL, so the room has to live in
  // the path — a query string would end up in the middle of the composed path.
  // The websocket cannot do this (it has no path to spare) and uses ?room= instead.
  const room = getRoom();
  return room ? `${origin}/r/${room}` : origin;
}

function resolveUrl(protocol: 'http' | 'ws', path: string) {
  const query = new URLSearchParams();
  const roomParam = getRoom();
  if (roomParam) {
    query.set('room', roomParam);
  }
  // a websocket handshake cannot carry an Authorization header
  const token = getToken();
  if (token) {
    query.set('token', token);
  }
  const querySuffix = query.toString() ? `?${query.toString()}` : '';

  const customWorkerUrl = import.meta.env.VITE_WORKER_URL;
  if (customWorkerUrl) {
    const cleanWorkerOrigin = customWorkerUrl.replace(/\/+$/, '');
    const cleanPath = path ? (path.startsWith('/') ? path : `/${path}`) : '';
    if (protocol === 'ws') {
      const wsOrigin = cleanWorkerOrigin.replace(/^http/, 'ws');
      return `${wsOrigin}${cleanPath}${querySuffix}`;
    }
    return `${cleanWorkerOrigin}${cleanPath}${querySuffix}`;
  }

  const url = new URL(window.location.origin);

  // generate ws url
  if (protocol === 'ws') {
    // ensure we remain in a secure context
    const isSecure = window.location.protocol === 'https:';
    url.protocol = isSecure ? 'wss' : 'ws';
  }

  // make path name relative to the base URI
  url.pathname = baseURI ? `${baseURI}/${path}` : path;

  // in development mode, we use the React port for UI, but need the requests to target the server
  // this is done with a proxy in the vite config to avoid CORS issues in the dev environment

  const result = url.toString();

  // prevent trailing slash
  return result.endsWith('/') ? result.slice(0, -1) : result;
}

/**
 * Resolves a base URI for a client that is not at the root segment
 * ie: https://cloud.getontime.com/client-hash/timer
 * This is necessary for ontime cloud and should otherwise not affect the client
 */
function resolveBaseURI(): string {
  // in ontime cloud, the base tag is set by the server
  const baseHref = document.querySelector('base')?.getAttribute('href');
  const base = baseHref ?? '';

  // prevent a trailing slash from either an empty base or a base with a trailing slash
  if (base.endsWith('/')) {
    return base.slice(0, -1);
  }

  return base;
}

/**
 * Resolves a session scope for the session
 */
export const sessionScope = resolveSessionScope();
export const getIsNavigationLocked = () => new URLSearchParams(window.location.search).get('n') === '1';

/**
 * The session scope is read from the cookie and will only exist if the app is password protected
 */
function resolveSessionScope() {
  const tokenCookie = document.cookie.split('; ').find((cookie) => cookie.startsWith('token='));

  if (tokenCookie) {
    try {
      const { scope } = JSON.parse(tokenCookie.split('=')[1]);
      return scope;
    } catch {
      return 'rw';
    }
  }
  return 'rw';
}
