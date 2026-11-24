import { useQuery } from '@tanstack/react-query';
import { langEn } from 'ontime-types';
import { MILLIS_PER_HOUR } from 'ontime-utils';

import { getUserTranslation } from '../../common/api/assets';
import { TRANSLATION } from '../../common/api/constants';
import { isRoomlessPage } from '../../externals';

export function useCustomTranslation() {
  const { data, status, refetch } = useQuery({
    queryKey: TRANSLATION,
    queryFn: ({ signal }) => getUserTranslation({ signal }),
    // a page without a room has no custom translation, and keeps English
    enabled: !isRoomlessPage,
    placeholderData: (previousData, _previousQuery) => previousData,
    staleTime: MILLIS_PER_HOUR,
  });
  return { data: data ?? langEn, status, refetch };
}
