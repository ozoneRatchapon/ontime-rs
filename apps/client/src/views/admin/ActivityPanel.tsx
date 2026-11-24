import { useCallback, useEffect, useState } from 'react';

import { ActivityAuth, ActivityLine, roomActivity } from '../../common/api/activity';
import Button from '../../common/components/buttons/Button';
import { describeActor, describeChange } from '../../common/utils/activityText';

import { formatDate, formatLastSeen } from './adminUtils';

import style from './Admin.module.scss';

interface ActivityPanelProps {
  room: string;
  auth: ActivityAuth;
}

/** Who changed what in a room, newest first. For its owners and the admin. */
export default function ActivityPanel({ room, auth }: ActivityPanelProps) {
  const [lines, setLines] = useState<ActivityLine[]>([]);
  const [before, setBefore] = useState<number | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // depend on the credential itself: callers build a fresh `auth` each render
  const session = 'session' in auth ? auth.session : null;
  const token = 'token' in auth ? auth.token : null;

  const load = useCallback(
    async (from: number | null) => {
      setLoading(true);
      try {
        const who: ActivityAuth = session !== null ? { session } : { token: token ?? '' };
        const page = await roomActivity(room, who, from);
        setLines((shown) => (from === null ? page.lines : [...shown, ...page.lines]));
        setBefore(page.before);
        setError(null);
      } catch (err) {
        setError(`Could not load the activity. ${err}`);
      } finally {
        setLoading(false);
      }
    },
    [room, session, token],
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
