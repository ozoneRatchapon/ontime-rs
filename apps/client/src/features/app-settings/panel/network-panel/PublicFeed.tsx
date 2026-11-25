import { useEffect, useState } from 'react';

import { getPublicFeed, PublicFeed as Feed, setPublicFeed } from '../../../../common/api/publicFeed';
import { maybeAxiosError } from '../../../../common/api/utils';
import Checkbox from '../../../../common/components/checkbox/Checkbox';
import CopyTag from '../../../../common/components/copy-tag/CopyTag';
import Switch from '../../../../common/components/switch/Switch';
import useCustomFields from '../../../../common/hooks-query/useCustomFields';
import { getRoom, getWorkerOrigin } from '../../../../externals';
import * as Panel from '../../panel-utils/PanelUtils';

/** The attendee page's address, and the feed's for other sites such as BeThere. */
export function publicUrls(origin: string, room: string) {
  const base = `${origin}/r/${encodeURIComponent(room)}/public`;
  return { page: `${base}/agenda/`, feed: base };
}

/**
 * On the Worker (plan 023): a live agenda attendees open on their phones,
 * without the room's token, which would let them change the room.
 */
export default function PublicFeed() {
  const room = getRoom() ?? 'default';
  const { data: customFields } = useCustomFields();
  const [feed, setFeed] = useState<Feed | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getPublicFeed()
      .then(setFeed)
      .catch((err) => setError(maybeAxiosError(err)));
  }, []);

  const change = async (next: Partial<Feed>) => {
    setError(null);
    try {
      setFeed(await setPublicFeed(next));
    } catch (err) {
      setError(maybeAxiosError(err));
    }
  };

  const toggleField = (key: string, shown: boolean) => {
    if (!feed) return;
    const fields = shown ? [...feed.fields, key] : feed.fields.filter((field) => field !== key);
    change({ fields });
  };

  const urls = publicUrls(getWorkerOrigin(), room);
  const fieldKeys = Object.keys(customFields);

  return (
    <Panel.Section>
      <Panel.Title>Attendee page</Panel.Title>
      <Panel.Description>
        A live agenda for attendees&apos; phones: what is on now, what comes next, and whether the show runs late. It
        needs no token and shows only titles, times and the fields picked below, never notes or skipped events.
      </Panel.Description>
      <Panel.InlineElements>
        <Switch
          checked={feed?.enabled ?? false}
          disabled={!feed}
          onCheckedChange={(enabled) => change({ enabled })}
          aria-label='Attendee page'
        />
        <span>{feed?.enabled ? 'On' : 'Off'}</span>
      </Panel.InlineElements>
      {feed && fieldKeys.length > 0 && (
        <>
          <Panel.Description>Custom fields attendees see, such as a presenter:</Panel.Description>
          <Panel.InlineElements wrap='wrap'>
            {fieldKeys.map((key) => (
              <label key={key} style={{ display: 'inline-flex', gap: '0.5rem', alignItems: 'center' }}>
                <Checkbox checked={feed.fields.includes(key)} onCheckedChange={(shown) => toggleField(key, shown)} />
                {customFields[key]?.label ?? key}
              </label>
            ))}
          </Panel.InlineElements>
        </>
      )}
      {feed?.enabled && (
        <>
          <Panel.Description>Share this with attendees (add ?lang=th for Thai):</Panel.Description>
          <CopyTag copyValue={urls.page}>{urls.page}</CopyTag>
          <Panel.Description>For another site, such as an event page, the same as JSON:</Panel.Description>
          <CopyTag copyValue={urls.feed}>{urls.feed}</CopyTag>
        </>
      )}
      {error && <Panel.Error>{error}</Panel.Error>}
    </Panel.Section>
  );
}
