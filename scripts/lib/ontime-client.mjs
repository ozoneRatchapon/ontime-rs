// What every script that talks to a cloud room needs: the token, the REST
// calls, the live playback state, and the time helpers.

import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

export const M = 60000;
export const H = 3600000;

export const DEFAULT_BASE = 'https://ontime-server-rs.solana-thailand.workers.dev';

/** The Keychain service the master password is kept under. */
const KEYCHAIN_SERVICE = 'ontime-session-password';

const repo_root = fileURLToPath(new URL('../../', import.meta.url));

/** "09:30" -> ms since midnight */
export function parse_hhmm(text) {
  const match = /^(\d{1,2})[:.](\d{2})$/.exec(String(text).trim());
  if (!match) throw new Error(`"${text}" is not a time — use HH:MM`);
  return Number(match[1]) * H + Number(match[2]) * M;
}

/** ms since midnight -> "09:30" */
export const hhmm = (ms) =>
  `${String(Math.floor(ms / H)).padStart(2, '0')}:${String(Math.floor((ms % H) / M)).padStart(2, '0')}`;

/** A custom field's key is its label with spaces turned into underscores (api.rs custom_field_key). */
export const field_key = (label) => label.trim().replaceAll(' ', '_');

/**
 * The master bearer token: an explicit one wins, then ONTIME_TOKEN, then it is
 * derived from the password in the macOS Keychain. The password itself never
 * leaves this function.
 */
export function resolve_token(explicit) {
  if (explicit) return explicit;
  if (process.env.ONTIME_TOKEN) return process.env.ONTIME_TOKEN;

  let password;
  try {
    password = execFileSync('security', ['find-generic-password', '-s', KEYCHAIN_SERVICE, '-w'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    }).replace(/\n$/, '');
  } catch {
    throw new Error(
      `no token: pass --token, set ONTIME_TOKEN, or store the master password in Keychain as "${KEYCHAIN_SERVICE}"`,
    );
  }

  return execFileSync(
    'cargo',
    ['run', '-q', '--manifest-path', 'packages/ontime-core/Cargo.toml', '--example', 'room-token'],
    { cwd: repo_root, input: password, encoding: 'utf8', stdio: ['pipe', 'pipe', 'inherit'] },
  ).trim();
}

/** REST calls against one room; throws on any 4xx/5xx with the server's message. */
export function room_api(base, room, token) {
  return (method, path, body) =>
    fetch(`${base}/r/${room}${path}`, {
      method,
      headers: {
        'content-type': 'application/json',
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
      },
      body: body ? JSON.stringify(body) : undefined,
    }).then(async (response) => {
      if (response.status >= 400) {
        throw new Error(`${method} ${path} -> ${response.status} ${await response.text()}`);
      }
      return response.status === 204 ? null : response.json().catch(() => null);
    });
}

/**
 * The room's playback ("stop", "play", "pause", "roll", "armed").
 *
 * There is no REST endpoint for it; a websocket gets the runtime straight after
 * connecting, so this opens one, reads that first packet, and closes.
 */
export function read_playback(base, room, token, timeout_ms = 8000) {
  const url = new URL(`${base.replace(/^http/, 'ws')}/r/${room}/ws`);
  if (token) url.searchParams.set('token', token);

  return new Promise((resolve, reject) => {
    const socket = new WebSocket(url);
    const timer = setTimeout(() => {
      socket.close();
      reject(new Error('no runtime state from the room within the timeout'));
    }, timeout_ms);

    socket.onmessage = (message) => {
      const packet = JSON.parse(message.data);
      if (packet.tag !== 'runtime-data') return;
      clearTimeout(timer);
      socket.close();
      resolve(packet.payload?.timer?.playback ?? 'stop');
    };
    socket.onerror = () => {
      clearTimeout(timer);
      reject(new Error('could not open the room websocket'));
    };
  });
}
