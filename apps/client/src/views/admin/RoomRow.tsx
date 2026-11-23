import { useState } from 'react';
import { IoLockClosed } from 'react-icons/io5';

import { RoomClient, RoomRecord, forgetRoom, saveRoomLabels } from '../../common/api/admin';
import Button from '../../common/components/buttons/Button';
import Input from '../../common/components/input/input/Input';

import { formatDate, formatLastSeen, roomLink, summariseClients } from './adminUtils';

import style from './Admin.module.scss';

interface RoomRowProps {
  room: RoomRecord;
  /** null while loading or when the room could not be asked */
  clients: RoomClient[] | null;
  onChanged: () => void;
  onError: (message: string) => void;
}

const VIEWS = ['editor', 'cuesheet', 'timer', 'backstage'];

export default function RoomRow({ room, clients, onChanged, onError }: RoomRowProps) {
  const [owner, setOwner] = useState(room.owner);
  const [note, setNote] = useState(room.note);

  /** Saves on leaving the field, and only what actually changed. */
  const save = async (field: 'owner' | 'note', value: string) => {
    if (value.trim() === room[field]) return;
    try {
      await saveRoomLabels(room.name, { [field]: value });
      onChanged();
    } catch (err) {
      onError(`Could not save ${field} of ${room.name}. ${err}`);
    }
  };

  const forget = async () => {
    const sure = window.confirm(
      `Remove "${room.name}" from this list?\n\nThe room and its data are kept — using it again lists it again.`,
    );
    if (!sure) return;
    try {
      await forgetRoom(room.name);
      onChanged();
    } catch (err) {
      onError(`Could not remove ${room.name}. ${err}`);
    }
  };

  return (
    <tr>
      <td>
        <a href={roomLink(room.name, 'editor')} target='_blank' rel='noreferrer' className={style.roomName}>
          {room.name}
        </a>
        {room.protected && <IoLockClosed className={style.lock} title='Has a room password' />}
        <div className={style.muted}>{room.title}</div>
      </td>
      <td>
        <Input
          value={owner}
          placeholder='Who looks after it'
          onChange={(event) => setOwner(event.target.value)}
          onBlur={() => save('owner', owner)}
          fluid
        />
      </td>
      <td>
        <Input
          value={note}
          placeholder='Note'
          onChange={(event) => setNote(event.target.value)}
          onBlur={() => save('note', note)}
          fluid
        />
      </td>
      <td title={formatDate(room.lastSeen)}>{formatLastSeen(room.lastSeen)}</td>
      <td>
        {clients === null ? (
          <span className={style.muted}>–</span>
        ) : clients.length === 0 ? (
          <span className={style.muted}>none</span>
        ) : (
          <span title={clients.map((client) => `${client.name} ${client.path}`).join('\n')}>
            <strong>{clients.length}</strong> {summariseClients(clients)}
          </span>
        )}
      </td>
      <td className={style.links}>
        {VIEWS.map((view) => (
          <a key={view} href={roomLink(room.name, view)} target='_blank' rel='noreferrer'>
            {view}
          </a>
        ))}
      </td>
      <td>
        <Button variant='ghosted-destructive' size='small' onClick={forget}>
          Remove
        </Button>
      </td>
    </tr>
  );
}
