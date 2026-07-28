import { useQuery } from '@tanstack/react-query';
import { ArchivedRun } from 'ontime-types';
import { MILLIS_PER_HOUR } from 'ontime-utils';

import { REPORT_ARCHIVE } from '../api/constants';
import { fetchReportArchive } from '../api/report';

export default function useReportArchive() {
  const { data, refetch } = useQuery<ArchivedRun[]>({
    queryKey: REPORT_ARCHIVE,
    queryFn: ({ signal }) => fetchReportArchive({ signal }),
    placeholderData: (previousData, _previousQuery) => previousData,
    staleTime: MILLIS_PER_HOUR,
  });

  return { data: data ?? [], refetch };
}
