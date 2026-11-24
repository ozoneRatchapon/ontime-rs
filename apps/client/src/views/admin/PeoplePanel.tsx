import { FormEvent, useState } from 'react';

import { PersonSummary, addPerson, createInvite, removePerson, signOutPerson } from '../../common/api/admin';
import Button from '../../common/components/buttons/Button';
import Input from '../../common/components/input/input/Input';
import { formatDate, formatLastSeen } from './adminUtils';

import style from './Admin.module.scss';

interface PeoplePanelProps {
  people: PersonSummary[];
  onChanged: () => void;
  onError: (message: string) => void;
}

/**
 * The people who can own rooms. Each signs in by opening a personal invite
 * link once — there is no password to hand out.
 */
export default function PeoplePanel({ people, onChanged, onError }: PeoplePanelProps) {
  const [name, setName] = useState('');
  // the link exists in the clear only in the response that made it
  const [invite, setInvite] = useState<{
    id: string;
    name: string;
    link: string;
    expiresAt: number;
    copied: boolean;
  } | null>(null);

  const add = async (event: FormEvent) => {
    event.preventDefault();
    if (!name.trim()) return;
    try {
      const person = await addPerson(name);
      setName('');
      onChanged();
      await makeInvite(person);
    } catch (err) {
      onError(`Could not add ${name}. ${err}`);
    }
  };

  const makeInvite = async ({ id, name }: { id: string; name: string }) => {
    try {
      const made = await createInvite(id);
      const copied = await navigator.clipboard
        .writeText(made.link)
        .then(() => true)
        .catch(() => false);
      setInvite({ id, name, ...made, copied });
      onChanged();
    } catch (err) {
      onError(`Could not make an invite link. ${err}`);
    }
  };

  const signOut = async (person: PersonSummary) => {
    if (!window.confirm(`Sign ${person.name} out of every device?\n\nAn unused invite link stops working too.`)) return;
    try {
      await signOutPerson(person.id);
      onChanged();
    } catch (err) {
      onError(`Could not sign ${person.name} out. ${err}`);
    }
  };

  const remove = async (person: PersonSummary) => {
    if (!window.confirm(`Remove ${person.name}?\n\nThey are signed out and taken off every room they own.`)) return;
    try {
      await removePerson(person.id);
      if (invite?.id === person.id) setInvite(null);
      onChanged();
    } catch (err) {
      onError(`Could not remove ${person.name}. ${err}`);
    }
  };

  return (
    <section className={style.section}>
      <h2>People</h2>
      <p className={style.muted}>
        Add a person, then send them their invite link (by LINE, for example). Opening it once signs that device in. A
        link works once and for 7 days.
      </p>

      {invite && (
        <div className={style.invite}>
          <div>
            Invite link for <strong>{invite.name}</strong>{' '}
            {invite.copied ? '— copied' : '— copy it now, it is shown only once'}
          </div>
          <code className={style.inviteLink}>{invite.link}</code>
          <div className={style.muted}>Works until {formatDate(invite.expiresAt)}</div>
        </div>
      )}

      <table className={style.table}>
        <thead>
          <tr>
            <th>Name</th>
            <th>Signed in on</th>
            <th>Last seen</th>
            <th>Invite</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {people.map((person) => (
            <tr key={person.id}>
              <td>{person.name}</td>
              <td>
                {person.devices.length > 0 ? person.devices.join(', ') : <span className={style.muted}>no device</span>}
              </td>
              <td title={formatDate(person.lastSeen)}>{formatLastSeen(person.lastSeen)}</td>
              <td>
                {person.inviteExpiresAt ? (
                  <span className={style.muted}>waiting, until {formatDate(person.inviteExpiresAt)}</span>
                ) : (
                  <span className={style.muted}>none</span>
                )}
              </td>
              <td className={style.actions}>
                <Button size='small' onClick={() => makeInvite(person)}>
                  {person.inviteExpiresAt ? 'New link' : 'Invite link'}
                </Button>
                <Button
                  variant='ghosted'
                  size='small'
                  onClick={() => signOut(person)}
                  disabled={person.devices.length === 0 && !person.inviteExpiresAt}
                >
                  Sign out
                </Button>
                <Button variant='ghosted-destructive' size='small' onClick={() => remove(person)}>
                  Remove
                </Button>
              </td>
            </tr>
          ))}
          {people.length === 0 && (
            <tr>
              <td colSpan={5} className={style.muted}>
                Nobody yet.
              </td>
            </tr>
          )}
        </tbody>
      </table>

      <form className={style.addRow} onSubmit={add}>
        <Input placeholder='Name, e.g. อ.สมศรี' value={name} onChange={(event) => setName(event.target.value)} />
        <Button type='submit' variant='primary' disabled={!name.trim()}>
          Add and make a link
        </Button>
      </form>
    </section>
  );
}
