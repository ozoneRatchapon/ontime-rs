import type { MouseEvent } from 'react';

import { activeRoom, baseURI, clientURL } from '../../externals';

type ElectronWindow = Window & {
  process?: { type?: string };
  ipcRenderer?: {
    send: (channel: string, args?: string | object) => void;
  };
};

/**
 * Open an external URLs: specifically for a electron / browser case
 * If electron: ask main process to call a new browser window
 * If browser: open in new tab
 */
export function openLink(url: string) {
  const electronWindow = window as ElectronWindow;
  if (electronWindow.process?.type === 'renderer' && electronWindow.ipcRenderer) {
    electronWindow.ipcRenderer.send('send-to-link', url);
  } else {
    window.open(url);
  }
}

/**
 * Opens one of our own views in a new window.
 *
 * Built from the client origin, not the API origin: every caller passes a view path
 * (`timer`, `rundown`, a preset alias), and when the client and API are on different
 * hosts the API serves no HTML. Upstream the two are the same, so this is unchanged
 * there.
 *
 * baseUrl, baseURI and room are parameters so the behaviour can be tested.
 */
export function handleLinks(
  location: string,
  event?: MouseEvent,
  externalClientUrl: string = clientURL,
  externalBaseURI: string = baseURI,
  room: string | null = activeRoom,
) {
  // we handle the link manually
  event?.preventDefault();
  openLink(getAppUrl(location, externalClientUrl, externalBaseURI, room));
}

/**
 * The full URL of a page of this Ontime app, respecting Ontime Clouds base URL
 * Built on the client's origin, which serves the pages here, and carrying the
 * room, since a new window is a fresh page load that would otherwise land in
 * the shared default project
 * externalClientUrl, externalBaseURI and room are parameters for testing
 */
export function getAppUrl(
  location: string,
  externalClientUrl: string = clientURL,
  externalBaseURI: string = baseURI,
  room: string | null = activeRoom,
): string {
  const destination = new URL(externalClientUrl);
  // split off any query string so it is not encoded into the pathname
  const [pathname, search] = location.split('?');
  destination.pathname = externalBaseURI ? `${externalBaseURI}/${pathname}` : pathname;
  if (search) {
    destination.search = search;
  }
  if (room && !destination.searchParams.has('room')) {
    destination.searchParams.set('room', room);
  }
  return destination.toString();
}

export function linkToOtherHost(
  host: string,
  path?: string,
  externalServerUrl: string = serverURL,
  externalBaseURI: string = baseURI,
) {
  const destination = new URL(externalServerUrl);
  destination.hostname = host;
  if (path) {
    destination.pathname = externalBaseURI ? `${externalBaseURI}/${path}` : path;
  }
  return destination.toString();
}

/**
 * Builds a base URL for reaching the Ontime server on a given network interface.
 * The Ontime server itself only speaks HTTP, so we only keep https when the page
 * already reaches the server over TLS on that same port (eg. a proxy in front of it);
 * any other explicit server port gets http.
 * externalServerUrl is used for testing
 */
export function hostToBaseUrl(host: string, port: number, externalServerUrl: string = serverURL): string {
  const page = new URL(externalServerUrl);
  const pagePort = page.port === '' ? (page.protocol === 'https:' ? 443 : 80) : Number(page.port);
  const scheme = page.protocol === 'https:' && pagePort === port ? 'https' : 'http';
  return `${scheme}://${host}:${port}`;
}
