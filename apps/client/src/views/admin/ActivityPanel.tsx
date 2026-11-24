import { useCallback, useEffect, useRef, useState } from 'react';

import { ActivityAuth, ActivityLine, ActivityPage, peopleActivity, roomActivity } from '../../common/api/activity';
import Button from '../../common/components/buttons/Button';
import { describeActor, describeChange } from '../../common/utils/activityText';

import { formatDate, formatLastSeen } from './adminUtils';

import style from './Admin.module.scss';

/** Whose log to show: one room's, or the people events (admin only). */
export type ActivitySource = { kind: 'room'; room: string; auth: ActivityAuth } | { kind: 'people'; token: string };

function fetchPage(source: ActivitySource, before: number | null): Promise<ActivityPage> {
  switch (source.kind) {
    case 'room':
      return roomActivity(source.room, source.auth, before);
    case 'people':
      return peopleActivity(source.token, before);
  }
}

/** A stable identity for a source, since callers build a fresh object each render. */
function sourceKey(source: ActivitySource): string {
  switch (source.kind) {
    case 'room':
      return JSON.stringify(['room', source.room, source.auth]);
    case 'people':
      return JSON.stringify(['people', source.token]);
  }
}

interface ActivityPanelProps {
  source: ActivitySource;
}

/** Who changed what, newest first. A room's for its owners and the admin; the people events for the admin. */
export default function ActivityPanel({ source }: ActivityPanelProps) {
  const [lines, setLines] = useState<ActivityLine[]>([]);
  const [before, setBefore] = useState<number | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // loads use the latest source, and reload only when its key changes
  const latest = useRef(source);
  latest.current = source;
  const key = sourceKey(source);

  const load = useCallback(
    async (from: number | null) => {
      setLoading(true);
      try {
        const page = await fetchPage(latest.current, from);
        setLines((shown) => (from === null ? page.lines : [...shown, ...page.lines]));
        setBefore(page.before);
        setError(null);
      } catch (err) {
        setError(`Could not load the activity. ${err}`);
      } finally {
        setLoading(false);
      }
    },
    [key],
  );

  useEffect(() => {
    load(null);
  }, [load]);

  return (
    <div className={style.team}>
      <div className={style.actions}>
        <strong>Activity</strong>
        <span className={style.muted}>what changed and who did it · field values are not kept</span>
        <Button size='small' variant='ghosted' onClick={() => load(null)} disabled={loading}>
          Refresh
        </Button>
      </div>

      {lines.length > 0 && (
        <table className={style.table}>
          <tbody>
            {lines.map((line, index) => (
              <tr key={`${line.at}-${index}`}>
                <td className={style.muted} title={formatDate(line.at)}>
                  {formatLastSeen(line.at)}
                </td>
                <td>{describeActor(line.actor)}</td>
                <td>{describeChange(line)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {!loading && !error && lines.length === 0 && <p className={style.muted}>Nothing recorded yet.</p>}

      {before !== null && (
        <Button size='small' onClick={() => load(before)} disabled={loading}>
          Show older
        </Button>
      )}
      {error && <p className={style.error}>{error}</p>}
    </div>
  );
}
