import { Fragment, useCallback, useEffect, useState } from 'react';
import { IoLockClosed, IoPeople } from 'react-icons/io5';

import { MemberRoom, Person } from '../../common/api/admin';
import { signOut, whoAmI } from '../../common/api/personSession';
import Button from '../../common/components/buttons/Button';
import { formatDate, formatLastSeen, roomLink } from '../admin/adminUtils';

import TeamPanel from './TeamPanel';

import style from '../admin/Admin.module.scss';

const VIEWS = ['editor', 'cuesheet', 'timer', 'backstage'];

type State =
  | { kind: 'loading' }
  | { kind: 'signed-out' }
  | { kind: 'error'; message: string }
  | { kind: 'ready'; person: Person; rooms: MemberRoom[] };

/** The rooms the person signed in on this device owns. */
export default function MyRooms() {
  const [state, setState] = useState<State>({ kind: 'loading' });
  // the room whose team is open, one at a time
  const [open, setOpen] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const me = await whoAmI();
      setState(me ? { kind: 'ready', ...me } : { kind: 'signed-out' });
    } catch (err) {
      setState({ kind: 'error', message: `Could not load your rooms. ${err}` });
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const leave = async () => {
    await signOut();
    setState({ kind: 'signed-out' });
  };

  switch (state.kind) {
    case 'loading':
      return <div className={style.admin} />;
    case 'signed-out':
      return (
        <div className={style.admin}>
          <div className={style.login}>
            <h1>My rooms</h1>
            <p className={style.muted}>This device is not signed in. Open the invite link the admin sent you.</p>
          </div>
        </div>
      );
    case 'error':
      return (
        <div className={style.admin}>
          <p className={style.error}>{state.message}</p>
          <Button onClick={load}>Try again</Button>
        </div>
      );
    case 'ready':
      return (
        <div className={style.admin}>
          <header className={style.header}>
            <div>
              <h1>My rooms</h1>
              <p className={style.muted}>Signed in as {state.person.name}</p>
            </div>
            <div className={style.actions}>
              <Button onClick={load}>Refresh</Button>
              <Button variant='ghosted' onClick={leave}>
                Sign out of this device
              </Button>
            </div>
          </header>

          <table className={style.table}>
            <thead>
              <tr>
                <th>Room</th>
                <th>You are</th>
                <th>Note</th>
                <th>Last used</th>
                <th>Open</th>
              </tr>
            </thead>
            <tbody>
              {state.rooms.map((room) => (
                <Fragment key={room.name}>
                <tr>
                  <td>
                    <a href={roomLink(room.name, 'editor')} className={style.roomName}>
                      {room.name}
                    </a>
                    {room.protected && <IoLockClosed className={style.lock} title='Has a room password' />}
                    {room.membersOnly && <IoPeople className={style.lock} title='Editing needs sign-in' />}
                    <div className={style.muted}>{room.title}</div>
                  </td>
                  <td>
                    {room.role}
                    {room.role === 'owner' && (
                      <div>
                        <Button size='small' variant='ghosted' onClick={() => setOpen(open === room.name ? null : room.name)}>
                          {open === room.name ? 'Hide team' : 'Team'}
                        </Button>
                      </div>
                    )}
                  </td>
                  <td>{room.note}</td>
                  <td title={formatDate(room.lastSeen)}>{formatLastSeen(room.lastSeen)}</td>
                  <td className={style.links}>
                    {VIEWS.map((view) => (
                      <a key={view} href={roomLink(room.name, view)} target='_blank' rel='noreferrer'>
                        {view}
                      </a>
                    ))}
                  </td>
                </tr>
                {open === room.name && (
                  <tr>
                    <td colSpan={5}>
                      <TeamPanel room={room} onChanged={load} />
                    </td>
                  </tr>
                )}
                </Fragment>
              ))}
              {state.rooms.length === 0 && (
                <tr>
                  <td colSpan={5} className={style.muted}>
                    No rooms yet. An admin or a room owner adds you to a room.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      );
  }
}
