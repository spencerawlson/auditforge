/** Transient notifications, and the hook screens use to raise them. */

import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react";

export type ToastTone = "info" | "success" | "warning" | "error";

interface Toast {
  id: number;
  tone: ToastTone;
  title?: string;
  message: string;
}

interface ToastApi {
  push: (message: string, options?: { tone?: ToastTone; title?: string }) => void;
  success: (message: string, title?: string) => void;
  error: (message: string, title?: string) => void;
  warning: (message: string, title?: string) => void;
}

const ToastContext = createContext<ToastApi | null>(null);

/** Errors stay up until dismissed; anything else clears itself. */
const AUTO_DISMISS_MS = 6000;

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);

  const dismiss = useCallback((id: number) => {
    setToasts((current) => current.filter((toast) => toast.id !== id));
  }, []);

  const push = useCallback<ToastApi["push"]>(
    (message, options) => {
      const id = Date.now() + Math.random();
      const tone = options?.tone ?? "info";
      setToasts((current) => [
        ...current.slice(-4),
        { id, tone, message, ...(options?.title ? { title: options.title } : {}) },
      ]);
      if (tone !== "error") {
        window.setTimeout(() => dismiss(id), AUTO_DISMISS_MS);
      }
    },
    [dismiss],
  );

  const api = useMemo<ToastApi>(
    () => ({
      push,
      success: (message, title) => push(message, { tone: "success", ...(title ? { title } : {}) }),
      error: (message, title) => push(message, { tone: "error", ...(title ? { title } : {}) }),
      warning: (message, title) => push(message, { tone: "warning", ...(title ? { title } : {}) }),
    }),
    [push],
  );

  return (
    <ToastContext.Provider value={api}>
      {children}
      <div className="toasts" role="region" aria-live="polite" aria-label="Notifications">
        {toasts.map((toast) => (
          <div key={toast.id} className={`toast toast--${toast.tone}`}>
            <div className="toast__body">
              {toast.title && <div className="toast__title">{toast.title}</div>}
              <div>{toast.message}</div>
            </div>
            <button
              type="button"
              className="toast__close"
              onClick={() => dismiss(toast.id)}
              aria-label="Dismiss"
            >
              ✕
            </button>
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  );
}

export function useToast(): ToastApi {
  const context = useContext(ToastContext);
  if (!context) throw new Error("useToast must be used inside a ToastProvider");
  return context;
}
