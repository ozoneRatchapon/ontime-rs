import { baseURI, getToken, isWorkerBackend, serverURL } from '../../../../externals';

export const customViewsDocs = 'https://docs.getontime.no/features/custom-views/';
const customViewSlugPattern = /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/;
// a Worker keeps each page as one stored value, which holds up to 2 MB (plan 021)
const maxUploadBytes = isWorkerBackend ? 1_000_000 : 4_000_000;
export const maxUploadLabel = `${maxUploadBytes / 1_000_000}MB`;

export function getSlugError(slug: string): string | null {
  if (!slug) {
    return 'Name is required.';
  }
  if (!customViewSlugPattern.test(slug)) {
    return 'Use lowercase letters, numbers, and dashes only.';
  }
  return null;
}

export function getFileError(file: File | null): string | null {
  if (!file) {
    return 'index.html is required.';
  }
  if (file.name.toLowerCase() !== 'index.html') {
    return 'Only index.html uploads are supported.';
  }
  if (file.size === 0) {
    return 'Uploaded file is empty.';
  }
  if (file.size > maxUploadBytes) {
    return `File size limit (${maxUploadLabel}) exceeded.`;
  }
  return null;
}

export function getViewUrl(slug: string, onWorker = isWorkerBackend, token = getToken()): string {
  if (onWorker) {
    // the room's pages are on the Worker, under the room's path, and a locked
    // room's page needs its token, which the page then carries to the room
    const url = new URL(`${serverURL}/external/${encodeURIComponent(slug)}/`);
    if (token) url.searchParams.set('token', token);
    return url.toString();
  }
  const url = new URL(serverURL);
  const path = slug ? `external/${encodeURIComponent(slug)}/` : 'external/';
  url.pathname = baseURI ? `${baseURI}/${path}` : `/${path}`;
  return url.toString();
}
