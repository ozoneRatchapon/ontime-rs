import { FormEvent, useCallback, useEffect, useState } from 'react';

import {
  AdminError,
  RoomClient,
  RoomRecord,
  clearAdminToken,
  getAdminToken,
  listRooms,
  roomClients,
  saveRoomLabels,
} from '../../common/api/admin';
import Button from '../../common/components/buttons/Button';
import Input from '../../common/components/input/input/Input';

import AdminLogin from './AdminLogin';
import RoomRow from './RoomRow';

import style from './Admin.module.scss';

/** Live screens change quickly; the list itself rarely does. */
const REFRESH_MS = 30_000;

/**
 * Every room on this Worker, who looks after each, and who is watching it now.
 *
 * Rooms list themselves when they are used; a room nobody has opened since the
 * list was introduced is added by hand with the form at the bottom.
 */
export default function Admin() {
  const [signedIn, setSignedIn] = useState(() => getAdminToken() !== null);

  if (!signedIn) {
    return (
      <div className={style.admin}>
        <AdminLogin onLogin={() => setSignedIn(true)} />
      </div>
    );
  }

  return (
    <RoomList
      onSignOut={() => {
        clearAdminToken();
        setSignedIn(false);
      }}
    />
  );
}

function RoomList({ onSignOut }: { onSignOut: () => void }) {
  const [rooms, setRooms] = useState<RoomRecord[] | null>(null);
  const [clients, setClients] = useState<Record<string, RoomClient[] | null>>({});
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const listed = await listRooms();
      setRooms(listed);
      setError(null);
      // each room answers for its own screens; one failing must not blank the rest
      const entries = await Promise.all(
        listed.map(async (room) => [room.name, await roomClients(room.name).catch(() => null)] as const),
      );
      setClients(Object.fromEntries(entries));
    } catch (err) {
      if (err instanceof AdminError && err.status === 401) {
        onSignOut();
        return;
      }
      setError(`Could not load the rooms. ${err}`);
    } finally {
      setLoading(false);
    }
  }, [onSignOut]);

  useEffect(() => {
    load();
    const timer = window.setInterval(load, REFRESH_MS);
    return () => window.clearInterval(timer);
  }, [load]);

  const watching = Object.values(clients).reduce((sum, list) => sum + (list?.length ?? 0), 0);

  return (
    <div className={style.admin}>
      <header className={style.header}>
        <div>
          <h1>Rooms</h1>
          {rooms && (
            <p className={style.muted}>
              {rooms.length} rooms · {watching} screens open now
            </p>
          )}
        </div>
        <div className={style.actions}>
          <Button onClick={load} loading={loading}>
            Refresh
          </Button>
          <Button variant='ghosted' onClick={onSignOut}>
            Sign out
          </Button>
        </div>
      </header>

      {error && <p className={style.error}>{error}</p>}

      {rooms && (
        <table className={style.table}>
          <thead>
            <tr>
              <th>Room</th>
              <th>Owner</th>
              <th>Note</th>
              <th>Last used</th>
              <th>Open now</th>
              <th>Open</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {rooms.map((room) => (
              <RoomRow
                // remount when the stored labels change, so the inputs show them
                key={`${room.name}:${room.owner}:${room.note}`}
                room={room}
                clients={clients[room.name] ?? null}
                onChanged={load}
                onError={setError}
              />
            ))}
            {rooms.length === 0 && (
              <tr>
                <td colSpan={7} className={style.muted}>
                  No rooms yet. A room appears here the first time it is used.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      )}

      <AddRoom onAdded={load} onError={setError} />
    </div>
  );
}

/** For a room that already existed before this list did. */
function AddRoom({ onAdded, onError }: { onAdded: () => void; onError: (message: string) => void }) {
  const [name, setName] = useState('');
  const [owner, setOwner] = useState('');
  const valid = /^[A-Za-z0-9_-]{1,64}$/.test(name);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!valid) return;
    try {
      await saveRoomLabels(name, { owner });
      setName('');
      setOwner('');
      onAdded();
    } catch (err) {
      onError(`Could not add ${name}. ${err}`);
    }
  };

  return (
    <form className={style.addRoom} onSubmit={submit}>
      <h2>Add an existing room</h2>
      <p className={style.muted}>
        Rooms list themselves when used. Add one here if it has not been opened since this list was introduced.
      </p>
      <div className={style.addRow}>
        <Input placeholder='room-name' value={name} onChange={(event) => setName(event.target.value.trim())} />
        <Input placeholder='Owner' value={owner} onChange={(event) => setOwner(event.target.value)} />
        <Button type='submit' variant='primary' disabled={!valid}>
          Add
        </Button>
      </div>
    </form>
  );
}
