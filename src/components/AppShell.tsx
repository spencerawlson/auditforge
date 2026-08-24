/** The application frame: sidebar navigation, top bar, global search and session state. */

import { useEffect, useMemo, useRef, useState } from "react";
import { NavLink, useNavigate } from "react-router-dom";

import { useDebouncedValue } from "@/hooks/useAsync";
import { SESSION_WARNING_SECS, useSession } from "@/features/auth/SessionProvider";
import { api } from "@/services/api";
import type { HitKind, SearchHit } from "@/types";

import { SeverityBadge } from "./badges";
import { Button } from "./ui";

const NAV_SECTIONS: Array<{
  heading: string;
  items: Array<{ to: string; label: string; icon: keyof typeof ICONS }>;
}> = [
  {
    heading: "Work",
    items: [
      { to: "/", label: "Dashboard", icon: "dashboard" },
      { to: "/engagements", label: "Engagements", icon: "briefcase" },
      { to: "/reports", label: "Reports", icon: "document" },
    ],
  },
  {
    heading: "Records",
    items: [
      { to: "/findings", label: "Findings", icon: "alert" },
      { to: "/evidence", label: "Evidence", icon: "paperclip" },
      { to: "/incidents", label: "Incidents", icon: "siren" },
      { to: "/postmortems", label: "Postmortems", icon: "clipboard" },
    ],
  },
  {
    heading: "Reference",
    items: [
      { to: "/templates", label: "Templates", icon: "layers" },
      { to: "/settings", label: "Settings", icon: "gear" },
    ],
  },
];

export function AppShell({ children }: { children: React.ReactNode }) {
  const { user, signOut, secondsRemaining, settings } = useSession();
  const navigate = useNavigate();

  // Theme and density come from settings and are applied to the document root, which is
  // where the CSS custom properties are defined.
  useEffect(() => {
    const root = document.documentElement;
    root.dataset.theme = settings?.ui.theme ?? "system";
    root.dataset.density = settings?.ui.compact_density ? "compact" : "comfortable";
  }, [settings?.ui.theme, settings?.ui.compact_density]);

  const expiringSoon = secondsRemaining !== null && secondsRemaining <= SESSION_WARNING_SECS;

  return (
    <div className="shell">
      <nav className="sidebar" aria-label="Main">
        <div className="sidebar__brand">
          <Shield />
          <span className="sidebar__name">AuditForge</span>
        </div>

        <div className="sidebar__nav">
          {NAV_SECTIONS.map((section) => (
            <div key={section.heading}>
              <div className="sidebar__group">{section.heading}</div>
              {section.items.map((item) => (
                <NavLink
                  key={item.to}
                  to={item.to}
                  end={item.to === "/"}
                  className={({ isActive }) => `navlink${isActive ? " navlink--active" : ""}`}
                >
                  <span className="navlink__icon">{ICONS[item.icon]}</span>
                  {item.label}
                </NavLink>
              ))}
            </div>
          ))}
        </div>

        <div className="sidebar__footer">
          <div className="stack stack--sm">
            <div className="small strong">{user?.full_name}</div>
            <div className="tiny muted">{user?.job_title || user?.username}</div>
            {user?.organization_name && <div className="tiny faint">{user.organization_name}</div>}
            <Button variant="ghost" size="sm" onClick={() => void signOut()}>
              Sign out
            </Button>
          </div>
        </div>
      </nav>

      <div className="main">
        <header className="topbar">
          <GlobalSearch onNavigate={(path) => navigate(path)} />
          <div className="topbar__spacer" />
          {expiringSoon && (
            <span className="badge badge--warning" title="Your session ends after a period of inactivity.">
              Session ends in {Math.max(1, Math.ceil((secondsRemaining ?? 0) / 60))} min
            </span>
          )}
          <AiIndicator />
        </header>
        <div className="content">{children}</div>
      </div>
    </div>
  );
}

/** Live privacy indicator, so the current AI mode is visible from every screen. */
function AiIndicator() {
  const [state, setState] = useState<{ mode: "offline" | "local" | "cloud"; text: string } | null>(
    null,
  );

  useEffect(() => {
    let cancelled = false;
    void api
      .aiStatus()
      .then((status) => {
        if (!cancelled) setState({ mode: status.privacy_mode, text: status.destination });
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);

  if (!state) return null;
  const label =
    state.mode === "cloud"
      ? "AI: OpenAI Cloud"
      : state.mode === "local"
        ? "AI: Local / Private"
        : "AI: Disabled";
  return (
    <span className={`privacy privacy--${state.mode}`} title={state.text}>
      <span className="privacy__dot" aria-hidden="true" />
      {label}
    </span>
  );
}

// ---------------------------------------------------------------- global search

const HIT_ROUTES: Record<HitKind, (id: string) => string> = {
  engagement: (id) => `/engagements/${id}`,
  report: (id) => `/reports/${id}`,
  finding: (id) => `/findings/${id}`,
  incident: (id) => `/incidents/${id}`,
  postmortem: (id) => `/postmortems/${id}`,
  evidence: () => `/evidence`,
};

const HIT_LABELS: Record<HitKind, string> = {
  engagement: "Engagement",
  report: "Report",
  finding: "Finding",
  incident: "Incident",
  postmortem: "Postmortem",
  evidence: "Evidence",
};

function GlobalSearch({ onNavigate }: { onNavigate: (path: string) => void }) {
  const [text, setText] = useState("");
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [open, setOpen] = useState(false);
  const [cursor, setCursor] = useState(0);
  const containerRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const query = useDebouncedValue(text, 200);

  useEffect(() => {
    if (query.trim().length < 2) {
      setHits([]);
      return;
    }
    let cancelled = false;
    void api
      .search({ text: query, limit: 12 })
      .then((results) => {
        if (cancelled) return;
        setHits(results);
        setCursor(0);
        setOpen(true);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [query]);

  // Ctrl/Cmd+K focuses search from anywhere.
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        inputRef.current?.focus();
        inputRef.current?.select();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  useEffect(() => {
    function onClick(event: MouseEvent) {
      if (!containerRef.current?.contains(event.target as Node)) setOpen(false);
    }
    document.addEventListener("mousedown", onClick);
    return () => document.removeEventListener("mousedown", onClick);
  }, []);

  const grouped = useMemo(() => hits.slice(0, 12), [hits]);

  function go(hit: SearchHit) {
    setOpen(false);
    setText("");
    onNavigate(HIT_ROUTES[hit.kind](hit.id));
  }

  return (
    <div className="topbar__search" ref={containerRef}>
      <input
        ref={inputRef}
        className="input"
        type="search"
        value={text}
        placeholder="Search engagements, reports, findings, evidence…   Ctrl+K"
        aria-label="Search everything"
        onChange={(event) => setText(event.target.value)}
        onFocus={() => hits.length > 0 && setOpen(true)}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            setOpen(false);
            return;
          }
          if (!open || grouped.length === 0) return;
          if (event.key === "ArrowDown") {
            event.preventDefault();
            setCursor((value) => Math.min(value + 1, grouped.length - 1));
          } else if (event.key === "ArrowUp") {
            event.preventDefault();
            setCursor((value) => Math.max(value - 1, 0));
          } else if (event.key === "Enter") {
            event.preventDefault();
            const hit = grouped[cursor];
            if (hit) go(hit);
          }
        }}
      />

      {open && grouped.length > 0 && (
        <div className="search-results" role="listbox">
          {grouped.map((hit, index) => (
            <button
              key={`${hit.kind}-${hit.id}`}
              type="button"
              role="option"
              aria-selected={index === cursor}
              data-active={index === cursor}
              className="search-result"
              onMouseEnter={() => setCursor(index)}
              onClick={() => go(hit)}
            >
              <span className="badge badge--outline">{HIT_LABELS[hit.kind]}</span>
              <span className="grow" style={{ minWidth: 0 }}>
                <span className="search-result__title table__truncate">{hit.title}</span>
                <span className="search-result__meta">
                  {hit.reference}
                  {hit.subtitle ? ` · ${hit.subtitle}` : ""}
                </span>
              </span>
              {hit.severity && <SeverityBadge severity={hit.severity} />}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------- icons

function Shield() {
  return (
    <svg className="sidebar__mark" viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path
        d="M12 2.2 4.4 5.1v6.4c0 4.6 3.1 8.9 7.6 10.3 4.5-1.4 7.6-5.7 7.6-10.3V5.1L12 2.2Z"
        fill="var(--accent-soft)"
        stroke="var(--accent)"
        strokeWidth="1.5"
        strokeLinejoin="round"
      />
      <path
        d="M8.4 10.2h7.2M9.6 13.6h4.8"
        stroke="var(--accent)"
        strokeWidth="1.6"
        strokeLinecap="round"
      />
    </svg>
  );
}

const stroke = {
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.6,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
};

const ICONS = {
  dashboard: (
    <svg viewBox="0 0 20 20" width="16" height="16" aria-hidden="true">
      <rect x="2.5" y="2.5" width="6" height="6" rx="1.4" {...stroke} />
      <rect x="11.5" y="2.5" width="6" height="6" rx="1.4" {...stroke} />
      <rect x="2.5" y="11.5" width="6" height="6" rx="1.4" {...stroke} />
      <rect x="11.5" y="11.5" width="6" height="6" rx="1.4" {...stroke} />
    </svg>
  ),
  briefcase: (
    <svg viewBox="0 0 20 20" width="16" height="16" aria-hidden="true">
      <rect x="2.5" y="6" width="15" height="11" rx="1.6" {...stroke} />
      <path d="M7 6V4.6A1.6 1.6 0 0 1 8.6 3h2.8A1.6 1.6 0 0 1 13 4.6V6" {...stroke} />
      <path d="M2.5 10.5h15" {...stroke} />
    </svg>
  ),
  document: (
    <svg viewBox="0 0 20 20" width="16" height="16" aria-hidden="true">
      <path d="M5 2.5h6.5L15.5 6.5V17a.5.5 0 0 1-.5.5H5a.5.5 0 0 1-.5-.5V3a.5.5 0 0 1 .5-.5Z" {...stroke} />
      <path d="M11.5 2.5v4h4M7 11h6M7 14h4" {...stroke} />
    </svg>
  ),
  alert: (
    <svg viewBox="0 0 20 20" width="16" height="16" aria-hidden="true">
      <path d="M10 3 2.8 16h14.4L10 3Z" {...stroke} />
      <path d="M10 8v3.4M10 13.8v.6" {...stroke} />
    </svg>
  ),
  paperclip: (
    <svg viewBox="0 0 20 20" width="16" height="16" aria-hidden="true">
      <path
        d="M14.5 9.3 9.7 14a3.2 3.2 0 0 1-4.5-4.5l5.4-5.4a2.1 2.1 0 1 1 3 3l-5.4 5.4a1 1 0 0 1-1.4-1.4l4.8-4.8"
        {...stroke}
      />
    </svg>
  ),
  siren: (
    <svg viewBox="0 0 20 20" width="16" height="16" aria-hidden="true">
      <path d="M5 14v-3a5 5 0 0 1 10 0v3" {...stroke} />
      <rect x="3.2" y="14" width="13.6" height="3.2" rx="1.2" {...stroke} />
      <path d="M10 3.2V2M4.2 5.4 3.3 4.5M15.8 5.4l.9-.9" {...stroke} />
    </svg>
  ),
  clipboard: (
    <svg viewBox="0 0 20 20" width="16" height="16" aria-hidden="true">
      <rect x="4.5" y="4" width="11" height="13.5" rx="1.4" {...stroke} />
      <path d="M7.6 4V3.2c0-.4.3-.7.7-.7h3.4c.4 0 .7.3.7.7V4" {...stroke} />
      <path d="M7.5 9h5M7.5 12.2h3.4" {...stroke} />
    </svg>
  ),
  layers: (
    <svg viewBox="0 0 20 20" width="16" height="16" aria-hidden="true">
      <path d="m10 2.8 7 3.6-7 3.6-7-3.6 7-3.6Z" {...stroke} />
      <path d="m3 10.4 7 3.6 7-3.6M3 14l7 3.6L17 14" {...stroke} />
    </svg>
  ),
  gear: (
    <svg viewBox="0 0 20 20" width="16" height="16" aria-hidden="true">
      <circle cx="10" cy="10" r="2.6" {...stroke} />
      <path
        d="M10 2.6v2M10 15.4v2M17.4 10h-2M4.6 10h-2M15.2 4.8 13.8 6.2M6.2 13.8l-1.4 1.4M15.2 15.2l-1.4-1.4M6.2 6.2 4.8 4.8"
        {...stroke}
      />
    </svg>
  ),
};
