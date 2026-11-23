import { FormEvent, useCallback, useEffect, useState } from 'react';

import { MemberRoom } from '../../common/api/admin';
import {
  TeamMember,
  addOperator,
  getTeam,
  relinkOperator,
  removeOperator,
  setMembersOnly,
} from '../../common/api/personSession';
import Button from '../../common/components/buttons/Button';
import Input from '../../common/components/input/input/Input';
import { formatDate, formatLastSeen } from '../admin/adminUtils';

import style from '../admin/Admin.module.scss';

interface TeamPanelProps {
  room: MemberRoom;
  onChanged: () => void;
}

interface Invite {
  name: string;
  link: string;
  expiresAt: number;
  copied: boolean;
}

async function copy(text: string): Promise<boolean> {
  return navigator.clipboard
    .writeText(text)
    .then(() => true)
    .catch(() => false);
}

/** An owner's view of one room: who runs it, and whether editing needs sign-in. */
export default function TeamPanel({ room, onChanged }: TeamPanelProps) {
  const [team, setTeam] = useState<{ owners: TeamMember[]; operators: TeamMember[] } | null>(null);
  const [name, setName] = useState('');
  const [invite, setInvite] = useState<Invite | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setTeam(await getTeam(room.name));
      setError(null);
    } catch (err) {
      setError(`Could not load the team. ${err}`);
    }
  }, [room.name]);

  useEffect(() => {
    load();
  }, [load]);

  const add = async (event: FormEvent) => {
    event.preventDefault();
    if (!name.trim()) return;
    try {
      const made = await addOperator(room.name, name);
      setInvite({ name: made.person.name, link: made.link, expiresAt: made.expiresAt, copied: await copy(made.link) });
      setName('');
      load();
    } catch (err) {
      setError(`Could not add ${name}. ${err}`);
    }
  };

  const relink = async ({ person }: TeamMember) => {
    try {
      const made = await relinkOperator(room.name, person.id);
      setInvite({ name: person.name, link: made.link, expiresAt: made.expiresAt, copied: await copy(made.link) });
      load();
    } catch (err) {
      setError(`Could not make a link for ${person.name}. ${err}`);
    }
  };

  const remove = async ({ person }: TeamMember) => {
    if (!window.confirm(`Remove ${person.name} from ${room.name}?`)) return;
    try {
      await removeOperator(room.name, person.id);
      load();
    } catch (err) {
      setError(`Could not remove ${person.name}. ${err}`);
    }
  };

  const toggle = async () => {
    const on = !room.membersOnly;
    const explain = on
      ? 'Only the owners and operators listed here (and anyone with the room password) will be able to edit and run the timer.\n\nTimer and backstage screens keep working without sign-in.'
      : 'Anyone who can open the room will be able to edit it again.';
    if (!window.confirm(`${on ? 'Turn on' : 'Turn off'} "editing needs sign-in" for ${room.name}?\n\n${explain}`)) return;
    try {
      await setMembersOnly(room.name, on);
      onChanged();
    } catch (err) {
      setError(`Could not change it. ${err}`);
    }
  };

  return (
    <div className={style.team}>
      <label className={style.switch}>
        <input type='checkbox' checked={room.membersOnly} onChange={toggle} />
        Editing needs sign-in
        <span className={style.muted}>
          {room.membersOnly ? 'only the people below can edit and run it' : 'anyone with the link can edit it'}
        </span>
      </label>

      {invite && (
        <div className={style.invite}>
          <div>
            Invite link for <strong>{invite.name}</strong>{' '}
            {invite.copied ? '— copied' : '— copy it now, it is shown only once'}
          </div>
          <code className={style.inviteLink}>{invite.link}</code>
          <div className={style.muted}>Works once, until {formatDate(invite.expiresAt)}</div>
        </div>
      )}

      {team && (
        <table className={style.table}>
          <tbody>
            {team.owners.map(({ person }) => (
              <tr key={`owner-${person.id}`}>
                <td>{person.name}</td>
                <td className={style.muted}>owner</td>
                <td className={style.muted}>{formatLastSeen(person.lastSeen)}</td>
                <td />
              </tr>
            ))}
            {team.operators.map((member) => (
              <tr key={`operator-${member.person.id}`}>
                <td>{member.person.name}</td>
                <td className={style.muted}>
                  operator
                  {member.person.inviteExpiresAt && ' · link not opened yet'}
                </td>
                <td className={style.muted}>{formatLastSeen(member.person.lastSeen)}</td>
                <td className={style.actions}>
                  {member.mine && (
                    <Button size='small' onClick={() => relink(member)}>
                      New link
                    </Button>
                  )}
                  <Button variant='ghosted-destructive' size='small' onClick={() => remove(member)}>
                    Remove
                  </Button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      <form className={style.addRow} onSubmit={add}>
        <Input placeholder='Operator name' value={name} onChange={(event) => setName(event.target.value)} />
        <Button type='submit' variant='primary' disabled={!name.trim()}>
          Add operator and make a link
        </Button>
      </form>

      {error && <p className={style.error}>{error}</p>}
    </div>
  );
}
