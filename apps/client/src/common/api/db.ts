import axios, { AxiosResponse } from 'axios';
import { DatabaseModel, MessageResponse, ProjectData, ProjectFileListResponse, QuickStartData } from 'ontime-types';

import { apiEntryUrl } from './constants';
import type { RequestOptions } from './requestOptions';
import { axiosConfig } from './requestTimeouts';
import { createBlob, downloadBlob } from './utils';

const dbPath = `${apiEntryUrl}/db`;

/**
 * HTTP request to the current DB
 */
export function getDb(filename: string): Promise<AxiosResponse<DatabaseModel>> {
  return axios.post(`${dbPath}/download`, { filename }, { timeout: axiosConfig.longTimeout });
}

/**
 * Request download of the current project file
 * @param fileName
 */
export async function downloadProject(fileName: string) {
  try {
    const { data, name } = await fileDownload(fileName);

    const fileContent = JSON.stringify(data, null, 2);

    const blob = createBlob(fileContent, 'application/json;charset=utf-8;');
    downloadBlob(blob, `${name}.json`);
  } catch (error) {
    console.error(error);
  }
}

/**
 * HTTP request to upload project file
 */
export async function uploadProjectFile(file: File): Promise<MessageResponse> {
  const formData = new FormData();
  formData.append('project', file);
  const response = await axios.post(`${dbPath}/upload`, formData, {
    timeout: axiosConfig.longTimeout,
    headers: {
      'Content-Type': 'multipart/form-data',
    },
  });
  return response.data;
}

/**
 * Make patch changes to the objects in the db
 */
export async function patchData(patchDb: Partial<DatabaseModel>): Promise<AxiosResponse<DatabaseModel>> {
  return await axios.patch(dbPath, patchDb);
}

/**
 * HTTP request to create a project file
 */
export async function createProject(
  project: Partial<
    ProjectData & {
      filename: string;
    }
  >,
): Promise<MessageResponse> {
  const res = await axios.post(`${dbPath}/new`, project);
  return res.data;
}

/**
 * HTTP request to create a project file
 */
export async function quickProject(data: QuickStartData): Promise<MessageResponse> {
  const res = await axios.post(`${dbPath}/quick`, data);
  return res.data;
}

/**
 * HTTP request to get the list of available project files
 */
export async function getProjects(options?: RequestOptions): Promise<ProjectFileListResponse> {
  const res = await axios.get(`${dbPath}/all`, {
    signal: options?.signal,
    timeout: options?.timeout,
  });
  return res.data;
}

/**
 * HTTP request to load a project file
 */
export async function loadProject(filename: string): Promise<MessageResponse> {
  const res = await axios.post(`${dbPath}/load`, {
    filename,
  });
  return res.data;
}

/**
 * HTTP request to load the demo project file
 */
export async function loadDemo(): Promise<MessageResponse> {
  const res = await axios.post(`${dbPath}/demo`);
  return res.data;
}

/**
 * HTTP request to duplicate a project file
 */
export async function duplicateProject(filename: string, newFilename: string): Promise<MessageResponse> {
  const url = `${dbPath}/${filename}/duplicate`;
  const decodedUrl = decodeURIComponent(url);
  const res = await axios.post(decodedUrl, {
    newFilename,
  });
  return res.data;
}

/**
 * HTTP request to rename a project file
 */
export async function renameProject(filename: string, newFilename: string): Promise<MessageResponse> {
  const url = `${dbPath}/${filename}/rename`;
  const decodedUrl = decodeURIComponent(url);
  const res = await axios.put(decodedUrl, {
    newFilename,
  });
  return res.data;
}

/**
 * HTTP request to delete a project file
 */
export async function deleteProject(filename: string): Promise<MessageResponse> {
  const url = `${dbPath}/${filename}`;
  const decodedUrl = decodeURIComponent(url);
  const res = await axios.delete(decodedUrl);
  return res.data;
}

/**
 * Utility function gets project from db
 * @param fileName
 * @returns
 */
async function fileDownload(fileName: string): Promise<{ data: DatabaseModel; name: string }> {
  const response = await getDb(fileName);

  const headerLine = response.headers['Content-Disposition'];

  // try and get the filename from the response
  let name = fileName;
  if (headerLine != null) {
    const startFileNameIndex = headerLine.indexOf('"') + 1;
    const endFileNameIndex = headerLine.lastIndexOf('"');
    name = headerLine.substring(startFileNameIndex, endFileNameIndex);
  }

  return { data: response.data, name };
}

/** What the Worker answers to an imported project file: what it had to drop or default. */
export type ImportedProjectFile = { imported: true; warnings: string[] };

/**
 * Worker only (plan 020): a project file from the Ontime app in place of this
 * room's show. Sent as its text, so the order of its rundowns survives.
 */
export async function importProjectFile(file: File): Promise<ImportedProjectFile> {
  const text = await file.text();
  const res = await axios.post(`${dbPath}/import`, { file: text }, { timeout: axiosConfig.longTimeout });
  return res.data;
}

/** Worker only (plan 020): downloads this room as a project file the Ontime app opens. */
export async function exportProjectFile(fileName: string) {
  const res = await axios.post(`${dbPath}/export`, null, { timeout: axiosConfig.longTimeout });
  const blob = createBlob(JSON.stringify(res.data, null, 2), 'application/json;charset=utf-8;');
  downloadBlob(blob, `${fileName}.json`);
}
