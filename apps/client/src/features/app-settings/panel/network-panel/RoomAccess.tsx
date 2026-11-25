import { useState } from 'react';

import { lockRoom, reopenRoom } from '../../../../common/api/session';
import Button from '../../../../common/components/buttons/Button';
import CopyTag from '../../../../common/components/copy-tag/CopyTag';
import useInfo from '../../../../common/hooks-query/useInfo';
import { getRoom, setToken } from '../../../../externals';
import * as Panel from '../../panel-utils/PanelUtils';

/**
 * Locks or reopens this room from the UI, so an operator never has to reach
 * for curl.
 *
 * A locked room needs its token on every request; the client keeps it in
 * sessionStorage keyed by room. Locking makes a new random token (plan 019):
 * there is no password to remember, only the link handed back here, which signs
 * a browser in once. Locking again makes a new link and retires the old one.
 */
export default function RoomAccess() {
  const { data, refetch } = useInfo();
  const isProtected = data.protected ?? false;
  const room = getRoom() ?? 'default';

  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [shareUrl, setShareUrl] = useState<string | null>(null);

  const lock = async () => {
    if (
      isProtected &&
      !window.confirm('Make a new link?\n\nEvery link shared before stops working, on every screen that uses one.')
    ) {
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const res = await lockRoom();
      setToken(res.token);
      const url = new URL(window.location.pathname, window.location.origin);
      url.searchParams.set('room', room);
      url.searchParams.set('token', res.token);
      setShareUrl(url.toString());
      await refetch();
    } catch (err) {
      setError(`Could not lock the room. ${err}`);
    } finally {
      setBusy(false);
    }
  };

  const reopen = async () => {
    setBusy(true);
    setError(null);
    try {
      await reopenRoom();
      setShareUrl(null);
      await refetch();
    } catch (err) {
      setError(`Could not reopen the room. ${err}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Panel.Section>
      <Panel.Title>Room access</Panel.Title>
      <Panel.Description>
        Room <Panel.Highlight>{room}</Panel.Highlight> is{' '}
        {isProtected
          ? 'locked — only people with its link (or signed-in members) can open it.'
          : 'open — anyone with the link can view and edit it.'}
      </Panel.Description>

      <Panel.InlineElements>
        <Button variant='primary' disabled={busy} onClick={lock}>
          {isProtected ? 'Make a new link' : 'Lock room'}
        </Button>
        {isProtected && (
          <Button variant='ghosted' disabled={busy} onClick={reopen}>
            Unlock room
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
