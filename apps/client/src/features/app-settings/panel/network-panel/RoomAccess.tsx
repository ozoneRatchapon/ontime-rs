import { useState } from 'react';

import { setRoomPassword } from '../../../../common/api/session';
import Button from '../../../../common/components/buttons/Button';
import CopyTag from '../../../../common/components/copy-tag/CopyTag';
import Input from '../../../../common/components/input/input/Input';
import useInfo from '../../../../common/hooks-query/useInfo';
import { getRoom, setToken } from '../../../../externals';
import * as Panel from '../../panel-utils/PanelUtils';

/**
 * Sets or removes this room's password from the UI, so an operator never has to
 * reach for curl.
 *
 * A locked room needs the token on every request; the client keeps it in
 * sessionStorage keyed by room. When a password is set the server returns the
 * token once — we store it so this session stays signed in, and hand back a
 * pre-authenticated link to share with the rest of the team.
 */
export default function RoomAccess() {
  const { data, refetch } = useInfo();
  const isProtected = data.protected ?? false;
  const room = getRoom() ?? 'default';

  const [password, setPassword] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [shareUrl, setShareUrl] = useState<string | null>(null);

  const protectRoom = async () => {
    if (!password) return;
    setBusy(true);
    setError(null);
    try {
      const res = await setRoomPassword(password);
      if (res.token) {
        setToken(res.token);
        const url = new URL(window.location.pathname, window.location.origin);
        url.searchParams.set('room', room);
        url.searchParams.set('token', res.token);
        setShareUrl(url.toString());
      }
      setPassword('');
      await refetch();
    } catch (err) {
      setError(`Could not set the password. ${err}`);
    } finally {
      setBusy(false);
    }
  };

  const reopenRoom = async () => {
    setBusy(true);
    setError(null);
    try {
      await setRoomPassword(null);
      setShareUrl(null);
      await refetch();
    } catch (err) {
      setError(`Could not remove the password. ${err}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Panel.Section>
      <Panel.Title>Room access</Panel.Title>
      <Panel.Description>
        Room <Panel.Highlight>{room}</Panel.Highlight> is{' '}
        {isProtected ? 'password protected.' : 'open — anyone with the link can view and edit it.'}
      </Panel.Description>

      <Panel.InlineElements>
        <Input
          type='password'
          value={password}
          fluid
          placeholder={isProtected ? 'New password' : 'Set a password'}
          disabled={busy}
          onChange={(event) => setPassword(event.target.value)}
        />
        <Button variant='primary' disabled={busy || !password} onClick={protectRoom}>
          {isProtected ? 'Change password' : 'Protect room'}
        </Button>
        {isProtected && (
          <Button variant='ghosted' disabled={busy} onClick={reopenRoom}>
            Remove password
          </Button>
        )}
      </Panel.InlineElements>

      {shareUrl && (
        <>
          <Panel.Description>
            Share this link with your team — it signs them in once, then their browser remembers it:
          </Panel.Description>
          <CopyTag copyValue={shareUrl}>{shareUrl}</CopyTag>
        </>
      )}
      {error && <Panel.Error>{error}</Panel.Error>}
    </Panel.Section>
  );
}
