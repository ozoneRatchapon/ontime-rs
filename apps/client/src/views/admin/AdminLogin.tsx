import { FormEvent, useState } from 'react';

import { AdminError, adminLogin } from '../../common/api/admin';
import Button from '../../common/components/buttons/Button';
import Input from '../../common/components/input/input/Input';

import style from './Admin.module.scss';

export default function AdminLogin({ onLogin }: { onLogin: () => void }) {
  const [password, setPassword] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!password) return;
    setBusy(true);
    setError(null);
    try {
      await adminLogin(password);
      setPassword('');
      onLogin();
    } catch (err) {
      setError(err instanceof AdminError && err.status === 401 ? 'Wrong password' : `Could not sign in. ${err}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <form className={style.login} onSubmit={submit}>
      <h1>Rooms</h1>
      <p className={style.muted}>Sign in with the master password to see every room.</p>
      <Input
        type='password'
        placeholder='Master password'
        value={password}
        onChange={(event) => setPassword(event.target.value)}
        autoFocus
        fluid
      />
      <Button type='submit' variant='primary' loading={busy} disabled={!password} fluid>
        Sign in
      </Button>
      {error && <p className={style.error}>{error}</p>}
    </form>
  );
}
