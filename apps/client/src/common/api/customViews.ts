import axios from 'axios';
import { type CustomViewsListResponse, type MessageResponse } from 'ontime-types';

import { isWorkerBackend } from '../../externals';
import { apiEntryUrl } from './constants';
import type { RequestOptions } from './requestOptions';
import { axiosConfig } from './requestTimeouts';
import { downloadBlob } from './utils';

const customViewsPath = `${apiEntryUrl}/custom-views`;

export async function getCustomViews(options?: RequestOptions): Promise<CustomViewsListResponse> {
  const response = await axios.get<CustomViewsListResponse>(customViewsPath, {
    signal: options?.signal,
    timeout: options?.timeout ?? axiosConfig.shortTimeout,
  });
  return response.data;
}

export async function uploadCustomView(slug: string, file: File, options?: RequestOptions): Promise<MessageResponse> {
  // the Worker takes the page as text in JSON, as it reads no multipart (plan 021)
  if (isWorkerBackend) {
    const html = await file.text();
    return (
      await axios.post<MessageResponse>(
        `${customViewsPath}/${encodeURIComponent(slug)}/upload`,
        { html },
        { signal: options?.signal, timeout: options?.timeout ?? axiosConfig.longTimeout },
      )
    ).data;
  }
  const formData = new FormData();
  formData.append('indexHtml', file);

  return (
    await axios.post<MessageResponse>(`${customViewsPath}/${encodeURIComponent(slug)}/upload`, formData, {
      signal: options?.signal,
      timeout: options?.timeout ?? axiosConfig.longTimeout,
      headers: {
        'Content-Type': 'multipart/form-data',
      },
    })
  ).data;
}

export async function downloadCustomView(slug: string, options?: RequestOptions): Promise<void> {
  const response = await axios.get(`${customViewsPath}/${encodeURIComponent(slug)}/download`, {
    signal: options?.signal,
    timeout: options?.timeout ?? axiosConfig.longTimeout,
    responseType: 'blob',
  });

  downloadBlob(response.data, `${slug}-index.html`);
}

export async function restoreDemoView(options?: RequestOptions): Promise<MessageResponse> {
  return (
    await axios.post<MessageResponse>(`${customViewsPath}/restore-demo`, null, {
      signal: options?.signal,
      timeout: options?.timeout ?? axiosConfig.longTimeout,
    })
  ).data;
}

export async function deleteCustomView(slug: string, options?: RequestOptions): Promise<void> {
  await axios.delete(`${customViewsPath}/${encodeURIComponent(slug)}`, {
    signal: options?.signal,
    timeout: options?.timeout ?? axiosConfig.longTimeout,
  });
}
