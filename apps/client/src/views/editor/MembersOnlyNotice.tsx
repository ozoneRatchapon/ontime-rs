import useInfo from '../../common/hooks-query/useInfo';

import style from './MembersOnlyNotice.module.scss';

/**
 * Says up front that this room only lets its members edit, instead of letting
 * every click fail. The rundown stays readable underneath.
 */
export default function MembersOnlyNotice() {
  const { data } = useInfo();
  if (!data.membersOnly || data.canEdit !== false) {
    return null;
  }
  return (
    <div className={style.notice} role='status'>
      This room needs sign-in to edit. You can watch it, but changes will not be saved. Ask the room&apos;s owner for an
      invite link.
    </div>
  );
}
