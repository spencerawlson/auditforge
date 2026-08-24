/**
 * The sign-in and account-creation screen.
 *
 * Both are always available from the toggle at the top; which one starts selected depends
 * on whether this machine already has an account. They live together because they share
 * the same frame and the same transition into the application.
 */

import { useEffect, useState, type FormEvent } from "react";

import { Button, Notice, TextField } from "@/components/ui";
import { api, BackendError } from "@/services/api";
import type { NewUser } from "@/types";

import { useSession } from "./SessionProvider";

type Mode = "create" | "login";

export function AuthScreen() {
  const { status, signIn, refreshStatus } = useSession();
  const [mode, setMode] = useState<Mode>("login");

  useEffect(() => {
    if (status && !status.has_account) setMode("create");
  }, [status]);

  return (
    <div className="auth">
      <aside className="auth__aside">
        <div className="auth__brand">
          <Wordmark />
          <span className="auth__brand-name">AuditForge</span>
        </div>

        <div className="stack stack--lg">
          <h1 className="auth__tagline">
            Structured security, compliance and operations reporting.
          </h1>
          <ul className="auth__points">
            <li>
              <Tick />
              <span>
                Answer guided questions. AuditForge handles section order, numbering,
                typography and pagination.
              </span>
            </li>
            <li>
              <Tick />
              <span>
                Findings, evidence and version history in one place, with a SHA-256 digest
                recorded for every file.
              </span>
            </li>
            <li>
              <Tick />
              <span>
                AI assistance is optional and off by default. It helps with language; the
                facts, severities and conclusions stay yours.
              </span>
            </li>
            <li>
              <Tick />
              <span>
                Everything is stored locally. Nothing leaves the machine unless you
                configure a cloud provider and turn it on.
              </span>
            </li>
          </ul>
        </div>

        <p className="tiny" style={{ color: "#8ea6bd" }}>
          {status ? `Version ${status.version}` : ""}
        </p>
      </aside>

      <main className="auth__panel">
        <div className="auth__form">
          {/* Both routes are always offered. Which one starts selected depends on whether
              this machine already has an account, but the choice is never taken away. */}
          <div className="segmented" role="tablist" aria-label="Sign in or create an account">
            <button
              type="button"
              role="tab"
              aria-selected={mode === "login"}
              className={`segmented__option${mode === "login" ? " segmented__option--active" : ""}`}
              onClick={() => setMode("login")}
            >
              Sign in
            </button>
            <button
              type="button"
              role="tab"
              aria-selected={mode === "create"}
              className={`segmented__option${mode === "create" ? " segmented__option--active" : ""}`}
              onClick={() => setMode("create")}
            >
              Create account
            </button>
          </div>
        </div>

        {mode === "create" ? (
          <CreateAccountForm
            hasExistingAccount={Boolean(status?.has_account)}
            onCreated={async () => {
              await refreshStatus();
              setMode("login");
            }}
          />
        ) : (
          <LoginForm onSignIn={signIn} hasAccount={Boolean(status?.has_account)} />
        )}
      </main>
    </div>
  );
}

// ---------------------------------------------------------------- login

function LoginForm({
  onSignIn,
  hasAccount,
}: {
  onSignIn: (username: string, password: string) => Promise<void>;
  hasAccount: boolean;
}) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [lockedFor, setLockedFor] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    setLockedFor(null);
    try {
      await onSignIn(username, password);
    } catch (raw) {
      const failure = raw as BackendError;
      if (failure.kind === "locked") {
        setLockedFor(failure.retryAfterSecs ?? 0);
        setError(null);
      } else if (failure.kind === "unauthorized") {
        // Deliberately does not say which half was wrong.
        setError("That username and password combination was not recognised.");
      } else {
        setError(failure.message);
      }
      setPassword("");
    } finally {
      setBusy(false);
    }
  }

  return (
    <form className="auth__form stack" onSubmit={submit}>
      <div>
        <h2 className="auth__title">Sign in</h2>
        <p className="auth__subtitle">Your reports and evidence stay on this machine.</p>
      </div>

      {!hasAccount && (
        <Notice tone="info" title="No account on this machine yet">
          Choose <span className="strong">Create account</span> above to make one. It becomes
          the default author on your reports.
        </Notice>
      )}

      {lockedFor !== null && (
        <Notice tone="danger" title="Account temporarily locked">
          Too many failed attempts. Try again in about {Math.max(1, Math.ceil(lockedFor / 60))}{" "}
          minute{Math.ceil(lockedFor / 60) === 1 ? "" : "s"}.
        </Notice>
      )}
      {error && <Notice tone="danger">{error}</Notice>}

      <TextField
        label="Username"
        value={username}
        onValueChange={setUsername}
        autoComplete="username"
        autoFocus
        required
      />
      <TextField
        label="Password"
        type="password"
        value={password}
        onValueChange={setPassword}
        autoComplete="current-password"
        required
      />

      <Button type="submit" variant="primary" size="lg" block busy={busy}>
        Sign in
      </Button>
    </form>
  );
}

// ---------------------------------------------------------------- create account

const EMPTY_ACCOUNT: NewUser = {
  username: "",
  password: "",
  confirm_password: "",
  full_name: "",
  organization: "",
  job_title: "",
  email: "",
};

function CreateAccountForm({
  onCreated,
  hasExistingAccount,
}: {
  onCreated: () => Promise<void>;
  hasExistingAccount: boolean;
}) {
  const [form, setForm] = useState<NewUser>(EMPTY_ACCOUNT);
  const [strength, setStrength] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  function update<K extends keyof NewUser>(key: K, value: NewUser[K]) {
    setForm((current) => ({ ...current, [key]: value }));
  }

  useEffect(() => {
    if (!form.password) {
      setStrength(0);
      return;
    }
    let cancelled = false;
    void api
      .passwordStrength(form.password)
      .then((score) => {
        if (!cancelled) setStrength(score);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [form.password]);

  const mismatch =
    form.confirm_password.length > 0 && form.password !== form.confirm_password
      ? "The two passwords do not match."
      : undefined;

  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await api.register(form);
      await onCreated();
    } catch (raw) {
      setError((raw as BackendError).message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <form className="auth__form stack" onSubmit={submit}>
      <div>
        <h2 className="auth__title">
          {hasExistingAccount ? "Create another account" : "Create your account"}
        </h2>
        <p className="auth__subtitle">
          This becomes the default author on every report you write. You can change it later;
          reports you have already finalized keep the details they were signed off with.
        </p>
      </div>

      {error && <Notice tone="danger">{error}</Notice>}

      <TextField
        label="Full name"
        value={form.full_name}
        onValueChange={(value) => update("full_name", value)}
        placeholder="Spencer Rawlson Spady"
        autoComplete="name"
        autoFocus
        required
      />
      <TextField
        label="Company or organization"
        value={form.organization}
        onValueChange={(value) => update("organization", value)}
        placeholder="PLH Ecosystem, Inc."
        autoComplete="organization"
      />
      <TextField
        label="Job title or credentials"
        value={form.job_title}
        onValueChange={(value) => update("job_title", value)}
        placeholder="Cloud Computing & Cybersecurity"
        autoComplete="organization-title"
      />
      <TextField
        label="Username"
        value={form.username}
        onValueChange={(value) => update("username", value)}
        help="Letters, digits, dot, underscore and hyphen."
        autoComplete="username"
        required
      />
      <div>
        <TextField
          label="Password"
          type="password"
          value={form.password}
          onValueChange={(value) => update("password", value)}
          help="At least 12 characters, combining three of: lowercase, uppercase, digits, symbols."
          autoComplete="new-password"
          required
        />
        <PasswordMeter score={strength} />
      </div>
      <TextField
        label="Confirm password"
        type="password"
        value={form.confirm_password}
        onValueChange={(value) => update("confirm_password", value)}
        {...(mismatch ? { error: mismatch } : {})}
        autoComplete="new-password"
        required
      />

      <Button type="submit" variant="primary" size="lg" block busy={busy}>
        Create account
      </Button>
    </form>
  );
}

function PasswordMeter({ score }: { score: number }) {
  const tone = score <= 1 ? "weak" : score === 2 ? "fair" : "strong";
  return (
    <div className="meter" aria-hidden="true">
      {[0, 1, 2, 3].map((index) => (
        <div
          key={index}
          className={`meter__bar${index < score ? ` meter__bar--${tone}` : ""}`}
        />
      ))}
    </div>
  );
}

// ---------------------------------------------------------------- marks

function Wordmark() {
  return (
    <svg width="26" height="26" viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path
        d="M12 2.2 4.4 5.1v6.4c0 4.6 3.1 8.9 7.6 10.3 4.5-1.4 7.6-5.7 7.6-10.3V5.1L12 2.2Z"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinejoin="round"
      />
      <path d="M8.4 10.2h7.2M9.6 13.6h4.8" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
    </svg>
  );
}

function Tick() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <path
        d="m3 8.5 3.2 3.2L13 5"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}
