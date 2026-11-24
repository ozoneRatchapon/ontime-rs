import { Fragment, useState } from 'react';
import { IoLockClosed, IoPeople } from 'react-icons/io5';

import {
  Person,
  RoomClient,
  RoomRecord,
  forgetRoom,
  getAdminToken,
  saveRoomLabels,
  saveRoomMembers,
} from '../../common/api/admin';
import Button from '../../common/components/buttons/Button';
import Input from '../../common/components/input/input/Input';
import ActivityPanel from './ActivityPanel';
import { formatDate, formatLastSeen, roomLink, summariseClients } from './adminUtils';
import OwnerPicker from './OwnerPicker';

import style from './Admin.module.scss';

interface RoomRowProps {
  room: RoomRecord;
  /** null while loading or when the room could not be asked */
  clients: RoomClient[] | null;
  people: Person[];
  onChanged: () => void;
  onError: (message: string) => void;
}

const VIEWS = ['editor', 'cuesheet', 'timer', 'backstage'];

/** The admin table's column count, for the activity row below a room. */
const COLUMNS = 7;

export default function RoomRow({ room, clients, people, onChanged, onError }: RoomRowProps) {
  const [owner, setOwner] = useState(room.owner);
  const [note, setNote] = useState(room.note);
  const [showActivity, setShowActivity] = useState(false);

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

  const saveMembers = async (members: { owners?: string[]; operators?: string[] }) => {
    try {
      await saveRoomMembers(room.name, members);
      onChanged();
    } catch (err) {
      onError(`Could not save who looks after ${room.name}. ${err}`);
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
    <Fragment>
      <tr>
        <td>
          <a href={roomLink(room.name, 'editor')} target='_blank' rel='noreferrer' className={style.roomName}>
            {room.name}
          </a>
          {room.protected && <IoLockClosed className={style.lock} title='Has a room password' />}
          {room.membersOnly && <IoPeople className={style.lock} title='Editing needs sign-in' />}
          <div className={style.muted}>{room.title}</div>
        </td>
        <td>
          <OwnerPicker owners={room.owners} people={people} onChange={(owners) => saveMembers({ owners })} />
          <div className={style.operators}>
            <span className={style.muted}>operators</span>
            <OwnerPicker
              owners={room.operators}
              people={people}
              label='operator'
              onChange={(operators) => saveMembers({ operators })}
            />
          </div>
          {/* the free-text owner from before people existed, until it is cleared */}
          {room.owner && (
            <Input
              className={style.legacyOwner}
              value={owner}
              title='Old free-text owner — clear it once the real owner is set'
              onChange={(event) => setOwner(event.target.value)}
              onBlur={() => save('owner', owner)}
              fluid
            />
          )}
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
          <div className={style.actions}>
            <Button size='small' variant='ghosted' onClick={() => setShowActivity((shown) => !shown)}>
              {showActivity ? 'Hide activity' : 'Activity'}
            </Button>
            <Button variant='ghosted-destructive' size='small' onClick={forget}>
              Remove
            </Button>
          </div>
        </td>
      </tr>
      {showActivity && (
        <tr>
          <td colSpan={COLUMNS}>
            <ActivityPanel source={{ kind: 'room', room: room.name, auth: { token: getAdminToken() ?? '' } }} />
          </td>
        </tr>
      )}
    </Fragment>
  );
}
