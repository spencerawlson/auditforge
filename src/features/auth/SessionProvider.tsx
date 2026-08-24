/**
 * Session state for the whole application.
 *
 * The session token itself lives in Rust and never reaches this file. What the frontend
 * tracks is only: who is signed in, what the workspace looks like, and how long the idle
 * window has left.
 */

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";

import { useInterval } from "@/hooks/useAsync";
import { api, onSignedOut } from "@/services/api";
import type { AppSettings, StartupStatus, User } from "@/types";

interface SessionValue {
  /** Null until the first status call returns. */
  status: StartupStatus | null;
  user: User | null;
  settings: AppSettings | null;
  /** Seconds of inactivity remaining, or null when signed out. */
  secondsRemaining: number | null;
  signIn: (username: string, password: string) => Promise<void>;
  signOut: () => Promise<void>;
  refreshUser: () => Promise<void>;
  refreshSettings: () => Promise<void>;
  applySettings: (settings: AppSettings) => void;
  refreshStatus: () => Promise<void>;
}

const SessionContext = createContext<SessionValue | null>(null);

/** How often the idle countdown is refreshed. */
const COUNTDOWN_INTERVAL_MS = 20_000;
/** Below this, the UI starts warning that the session is about to end. */
export const SESSION_WARNING_SECS = 120;

export function SessionProvider({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<StartupStatus | null>(null);
  const [user, setUser] = useState<User | null>(null);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [secondsRemaining, setSecondsRemaining] = useState<number | null>(null);

  const clearSession = useCallback(() => {
    setUser(null);
    setSettings(null);
    setSecondsRemaining(null);
  }, []);

  const refreshStatus = useCallback(async () => {
    const next = await api.startupStatus();
    setStatus(next);
    setUser(next.user);
    if (!next.signed_in) clearSession();
  }, [clearSession]);

  const refreshSettings = useCallback(async () => {
    setSettings(await api.getSettings());
  }, []);

  const refreshUser = useCallback(async () => {
    setUser(await api.currentUser());
  }, []);

  useEffect(() => {
    void refreshStatus();
  }, [refreshStatus]);

  // Any command that discovers the session has ended tells us here, so every screen
  // returns to the login page at once rather than each showing its own error.
  useEffect(() => onSignedOut(clearSession), [clearSession]);

  useEffect(() => {
    if (!user) return;
    void refreshSettings();
  }, [user, refreshSettings]);

  useInterval(
    () => {
      void api
        .sessionRemaining()
        .then((value) => {
          setSecondsRemaining(value);
          if (value === null) clearSession();
        })
        .catch(() => clearSession());
    },
    user ? COUNTDOWN_INTERVAL_MS : null,
  );

  const signIn = useCallback(async (username: string, password: string) => {
    const result = await api.login(username, password);
    setUser(result.user);
    setStatus((current) =>
      current ? { ...current, has_account: true, signed_in: true, user: result.user } : current,
    );
    setSettings(await api.getSettings());
    setSecondsRemaining(await api.sessionRemaining());
  }, []);

  const signOut = useCallback(async () => {
    try {
      await api.logout();
    } finally {
      clearSession();
      setStatus((current) => (current ? { ...current, signed_in: false, user: null } : current));
    }
  }, [clearSession]);

  const value = useMemo<SessionValue>(
    () => ({
      status,
      user,
      settings,
      secondsRemaining,
      signIn,
      signOut,
      refreshUser,
      refreshSettings,
      applySettings: setSettings,
      refreshStatus,
    }),
    [status, user, settings, secondsRemaining, signIn, signOut, refreshUser, refreshSettings, refreshStatus],
  );

  return <SessionContext.Provider value={value}>{children}</SessionContext.Provider>;
}

export function useSession(): SessionValue {
  const context = useContext(SessionContext);
  if (!context) throw new Error("useSession must be used inside a SessionProvider");
  return context;
}

/** The signed-in user, for screens that only render behind the auth gate. */
export function useCurrentUser(): User {
  const { user } = useSession();
  if (!user) throw new Error("useCurrentUser used outside an authenticated route");
  return user;
}
