import { useState, type FormEvent } from 'react';
import { login } from '../store/app';
import { BrandMark } from '../components/Layout';
import { Button, ErrorNote, Field, Input } from '../components/ui';
import './login.css';

export function LoginPage() {
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await login(username, password);
    } catch (err) {
      setError(err);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="login">
      <form className="login-card" onSubmit={submit}>
        <div className="login-brand">
          <BrandMark size={26} />
          <span className="brand-name">cseek</span>
        </div>
        <div className="stack-sm">
          <h1 className="login-title">Sign in to the web UI</h1>
          <p className="small muted">
            Use the <code>[web.auth]</code> login from config.toml, not your Soulseek account. That one goes in Options once you're in.
          </p>
        </div>
        <Field label="Username">
          {(id) => <Input id={id} icon="user" autoFocus autoComplete="username" value={username} onChange={(e) => setUsername(e.target.value)} required />}
        </Field>
        <Field label="Password">
          {(id) => (
            <Input id={id} icon="lock" type="password" autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} required />
          )}
        </Field>
        <ErrorNote error={error} />
        <Button type="submit" variant="primary" size="lg" busy={busy} iconRight="arrowRight">
          Sign in
        </Button>
      </form>
    </div>
  );
}
