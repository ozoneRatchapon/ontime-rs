import { IoClose } from 'react-icons/io5';

import { Person } from '../../common/api/admin';

import style from './Admin.module.scss';

interface OwnerPickerProps {
  owners: string[];
  people: Person[];
  onChange: (owners: string[]) => void;
}

/** The people who own a room, as removable chips plus a list to add one. */
export default function OwnerPicker({ owners, people, onChange }: OwnerPickerProps) {
  const named = new Map(people.map((person) => [person.id, person.name]));
  const addable = people.filter((person) => !owners.includes(person.id));

  return (
    <div className={style.owners}>
      {owners.map((id) => (
        <span key={id} className={style.chip}>
          {named.get(id) ?? 'unknown'}
          <button
            type='button'
            className={style.chipRemove}
            aria-label={`Remove ${named.get(id) ?? 'owner'}`}
            onClick={() => onChange(owners.filter((owner) => owner !== id))}
          >
            <IoClose />
          </button>
        </span>
      ))}
      {addable.length > 0 && (
        <select
          className={style.addOwner}
          value=''
          aria-label='Add an owner'
          onChange={(event) => event.target.value && onChange([...owners, event.target.value])}
        >
          <option value=''>+ owner</option>
          {addable.map((person) => (
            <option key={person.id} value={person.id}>
              {person.name}
            </option>
          ))}
        </select>
      )}
      {owners.length === 0 && addable.length === 0 && <span className={style.muted}>add people below</span>}
    </div>
  );
}
