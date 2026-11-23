import { useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router';

import { AdminError } from '../../common/api/admin';
import { redeemInvite } from '../../common/api/personSession';

import style from '../admin/Admin.module.scss';

/**
 * Where an invite link lands: `/join#code=…`. Spends the code, remembers this
 * device, and moves on to the person's rooms.
 */
export default function Join() {
  const navigate = useNavigate();
  const [error, setError] = useState<string | null>(null);
  // strict mode runs effects twice, and a code only works once
  const started = useRef(false);

  useEffect(() => {
    if (started.current) return;
    started.current = true;

    const code = new URLSearchParams(window.location.hash.slice(1)).get('code');
    // the code must not linger in the address bar or the history
    window.history.replaceState(null, '', window.location.pathname);
    if (!code) {
      setError('This link has no invite code in it.');
      return;
    }
    redeemInvite(code)
      .then(() => navigate('/my-rooms', { replace: true }))
      .catch((err) =>
        setError(
          err instanceof AdminError && err.status === 401
            ? 'This invite link has already been used or has expired. Ask the admin for a new one.'
            : `Could not sign in. ${err}`,
        ),
      );
  }, [navigate]);

  return (
    <div className={style.admin}>
      <div className={style.login}>
        <h1>Signing in</h1>
        {error ? <p className={style.error}>{error}</p> : <p className={style.muted}>One moment…</p>}
      </div>
    </div>
  );
}
