import axios from 'axios';
import { ArchivedRun, OntimeReport } from 'ontime-types';

import { ontimeQueryClient } from '../../common/queryClient';
import { REPORT, apiEntryUrl } from './constants';
import type { RequestOptions } from './requestOptions';

export const reportUrl = `${apiEntryUrl}/report`;

/**
 * HTTP request to fetch all reports
 */
export async function fetchReport(options?: RequestOptions): Promise<OntimeReport> {
  const res = await axios.get(reportUrl, { signal: options?.signal });
  return res.data;
}

/**
 * HTTP request to fetch the permanent run archive
 */
export async function fetchReportArchive(options?: RequestOptions): Promise<ArchivedRun[]> {
  const res = await axios.get(`${reportUrl}/archive`, { signal: options?.signal });
  return res.data;
}

export async function deleteReport(id: string) {
  await axios.delete(`${reportUrl}/${id}`);
  await ontimeQueryClient.invalidateQueries({ queryKey: REPORT });
}

export async function deleteAllReport() {
  await axios.delete(`${reportUrl}/all`);
  await ontimeQueryClient.invalidateQueries({ queryKey: REPORT });
}
