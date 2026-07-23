import { useEffect } from 'react';
import { useLocation, useNavigate } from 'react-router';

import { activeRoom } from '../../externals';

/**
 * Keeps `?room=` in the address bar as the user moves between views.
 *
 * The room identifies which isolated project the app is talking to. It is read once
 * at load, so navigation that drops it still *works* for the current session — which
 * is exactly what makes losing it dangerous: the URL keeps functioning for whoever is
 * already in the session, and silently points at the shared default room for anyone
 * they send it to.
 *
 * Rather than patch every `navigate()` call site — several exist and more will be
 * added — this repairs the URL centrally after any navigation that dropped it.
 */
export default function useStickyRoom() {
  const location = useLocation();
  const navigate = useNavigate();

  useEffect(() => {
    // the room the app booted with. A session that started without one is genuinely
    // in the default room, so nothing is invented here.
    if (!activeRoom) {
      return;
    }
    const room = activeRoom;

    const params = new URLSearchParams(location.search);
    if (params.get('room') === room) {
      return;
    }

    params.set('room', room);
    // replace, so repairing the URL does not add a history entry the user
    // would have to press back through twice
    navigate({ pathname: location.pathname, search: params.toString() }, { replace: true });
  }, [location.pathname, location.search, navigate]);
}
