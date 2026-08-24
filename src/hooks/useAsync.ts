/** Small data-loading helpers. Enough structure for this app without a query library. */

import { useCallback, useEffect, useRef, useState } from "react";

import { BackendError } from "@/services/api";

export interface AsyncState<T> {
  data: T | null;
  error: BackendError | null;
  loading: boolean;
  /** Re-run the loader. Safe to call from event handlers. */
  reload: () => void;
  /** Replace the data locally, for example after a mutation returns the new record. */
  set: (value: T) => void;
}

/**
 * Run `loader` whenever `deps` change.
 *
 * Results from a superseded run are discarded, so a slow request cannot overwrite a
 * newer one, and nothing is written to state after unmount.
 */
export function useAsync<T>(loader: () => Promise<T>, deps: unknown[]): AsyncState<T> {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<BackendError | null>(null);
  const [loading, setLoading] = useState(true);
  const [nonce, setNonce] = useState(0);
  const runId = useRef(0);

  // The loader is usually an inline closure; keeping it in a ref means callers do not
  // have to memoize it for the effect to behave.
  const loaderRef = useRef(loader);
  loaderRef.current = loader;

  useEffect(() => {
    const id = ++runId.current;
    let cancelled = false;
    setLoading(true);

    loaderRef
      .current()
      .then((value) => {
        if (cancelled || id !== runId.current) return;
        setData(value);
        setError(null);
      })
      .catch((raw: unknown) => {
        if (cancelled || id !== runId.current) return;
        setError(raw instanceof BackendError ? raw : new BackendError({ kind: "internal", message: String(raw) }));
      })
      .finally(() => {
        if (cancelled || id !== runId.current) return;
        setLoading(false);
      });

    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...deps, nonce]);

  const reload = useCallback(() => setNonce((value) => value + 1), []);
  const set = useCallback((value: T) => setData(value), []);

  return { data, error, loading, reload, set };
}

/**
 * Wrap a mutating call: tracks the in-flight state and surfaces the error, so a screen
 * does not need its own `busy` boolean per button.
 */
export function useAction<Args extends unknown[], T>(
  action: (...args: Args) => Promise<T>,
): [(...args: Args) => Promise<T | undefined>, { busy: boolean; error: BackendError | null }] {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<BackendError | null>(null);
  const actionRef = useRef(action);
  actionRef.current = action;

  const run = useCallback(async (...args: Args) => {
    setBusy(true);
    setError(null);
    try {
      return await actionRef.current(...args);
    } catch (raw: unknown) {
      setError(
        raw instanceof BackendError ? raw : new BackendError({ kind: "internal", message: String(raw) }),
      );
      return undefined;
    } finally {
      setBusy(false);
    }
  }, []);

  return [run, { busy, error }];
}

/** Value that settles `delayMs` after the last change. Used by the search box. */
export function useDebouncedValue<T>(value: T, delayMs = 220): T {
  const [settled, setSettled] = useState(value);
  useEffect(() => {
    const timer = window.setTimeout(() => setSettled(value), delayMs);
    return () => window.clearTimeout(timer);
  }, [value, delayMs]);
  return settled;
}

/** Run `callback` on an interval, without restarting it when the callback identity changes. */
export function useInterval(callback: () => void, intervalMs: number | null) {
  const saved = useRef(callback);
  saved.current = callback;

  useEffect(() => {
    if (intervalMs === null) return;
    const timer = window.setInterval(() => saved.current(), intervalMs);
    return () => window.clearInterval(timer);
  }, [intervalMs]);
}
