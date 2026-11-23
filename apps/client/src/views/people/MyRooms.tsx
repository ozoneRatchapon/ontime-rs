import { useCallback, useEffect, useState } from 'react';
import { IoLockClosed } from 'react-icons/io5';

import { Person, RoomRecord } from '../../common/api/admin';
import { signOut, whoAmI } from '../../common/api/personSession';
import Button from '../../common/components/buttons/Button';
import { formatDate, formatLastSeen, roomLink } from '../admin/adminUtils';

import style from '../admin/Admin.module.scss';

const VIEWS = ['editor', 'cuesheet', 'timer', 'backstage'];

type State =
  | { kind: 'loading' }
  | { kind: 'signed-out' }
  | { kind: 'error'; message: string }
  | { kind: 'ready'; person: Person; rooms: RoomRecord[] };

/** The rooms the person signed in on this device owns. */
export default function MyRooms() {
  const [state, setState] = useState<State>({ kind: 'loading' });

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
                <th>Note</th>
                <th>Last used</th>
                <th>Open</th>
              </tr>
            </thead>
            <tbody>
              {state.rooms.map((room) => (
                <tr key={room.name}>
                  <td>
                    <a href={roomLink(room.name, 'editor')} className={style.roomName}>
                      {room.name}
                    </a>
                    {room.protected && <IoLockClosed className={style.lock} title='Has a room password' />}
                    <div className={style.muted}>{room.title}</div>
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
              ))}
              {state.rooms.length === 0 && (
                <tr>
                  <td colSpan={4} className={style.muted}>
                    No rooms yet. The admin assigns rooms to you.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      );
  }
}
