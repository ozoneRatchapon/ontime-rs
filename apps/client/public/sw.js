// Retires the service worker that 142bc3efa registered on 2026-07-22.
//
// That worker answered every request from its cache first, under a cache name
// that never changed, so a browser which installed it kept showing the old app
// after every deploy: a normal reload got the old version and only a hard
// reload got the new one. The registration was removed a few hours later
// (bc8ae6270), but an installed worker stays until it is replaced, and the
// browser only replaces it when this file changes.
//
// So this file is the replacement. It installs at once, clears every cache the
// old worker made, unregisters itself and reloads any open page, once. After
// that the browser has no worker for this site and loads it from the network
// like any other page. Nothing registers a worker any more; keep this file
// served so browsers that still have the old one keep finding it.

self.addEventListener('install', () => {
  self.skipWaiting();
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    (async () => {
      const names = await caches.keys();
      await Promise.all(names.map((name) => caches.delete(name)));
      await self.registration.unregister();
      const pages = await self.clients.matchAll({ type: 'window' });
      // the page was served by the old worker; reload it from the network
      await Promise.all(pages.map((page) => page.navigate(page.url)));
    })(),
  );
});
