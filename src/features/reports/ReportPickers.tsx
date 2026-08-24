/** The findings and evidence sections of the report editor: what the document includes. */

import { useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";

import { SeverityBadge, SeverityConfirmationBadge } from "@/components/badges";
import { useToast } from "@/components/Toast";
import { Button, EmptyState, LoadingBlock, Notice } from "@/components/ui";
import { useAction, useAsync } from "@/hooks/useAsync";
import { api } from "@/services/api";
import { formatBytes } from "@/services/format";
import type { ReportWorkspace } from "@/types";

export function FindingPicker({
  workspace,
  editable,
  onChanged,
}: {
  workspace: ReportWorkspace;
  editable: boolean;
  onChanged: () => Promise<void>;
}) {
  const navigate = useNavigate();
  const toast = useToast();
  const [showPicker, setShowPicker] = useState(false);

  const all = useAsync(() => api.listFindings({ limit: 500 }), [showPicker]);
  const includedRefs = useMemo(
    () => new Set(workspace.version.content.findings.map((finding) => finding.finding_ref)),
    [workspace.version.content.findings],
  );

  const [setFindings, state] = useAction(async (ids: string[]) => {
    await api.saveReportDraft(workspace.report.id, { finding_ids: ids });
    await onChanged();
    return true;
  });

  const includedIds = useMemo(
    () =>
      (all.data ?? [])
        .filter((finding) => includedRefs.has(finding.finding_ref))
        .map((finding) => finding.id),
    [all.data, includedRefs],
  );

  const unconfirmed = (all.data ?? []).filter(
    (finding) => includedRefs.has(finding.finding_ref) && !finding.severity_confirmed,
  );

  return (
    <div className="stack">
      <p className="small muted">
        Findings included here are copied into the report when you save. A finalized report keeps
        its own copy, so editing or deleting a finding later cannot change a document that has
        already been issued.
      </p>

      {state.error && <Notice tone="danger">{state.error.message}</Notice>}

      {unconfirmed.length > 0 && (
        <Notice tone="warning" title="Severity not confirmed">
          {unconfirmed.map((finding) => finding.finding_ref).join(", ")} still need a severity
          confirmed by a person before this report can be finalized.
        </Notice>
      )}

      {workspace.version.content.findings.length === 0 ? (
        <EmptyState
          title="No findings included"
          hint="Add findings from your library, or create one first."
          action={
            editable ? (
              <Button variant="primary" onClick={() => setShowPicker(true)}>
                Choose findings
              </Button>
            ) : undefined
          }
        />
      ) : (
        <>
          <table className="table">
            <thead>
              <tr>
                <th>Severity</th>
                <th>Reference</th>
                <th>Title</th>
                <th>Affected asset</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {workspace.version.content.findings.map((finding) => (
                <tr key={finding.finding_ref}>
                  <td>
                    <SeverityBadge severity={finding.severity} />
                  </td>
                  <td className="table__ref">{finding.finding_ref}</td>
                  <td className="table__main table__truncate">{finding.title}</td>
                  <td className="muted table__truncate">{finding.affected_asset || "—"}</td>
                  <td className="right">
                    {editable && (
                      <Button
                        size="sm"
                        variant="ghost"
                        onClick={async () => {
                          const remaining = (all.data ?? [])
                            .filter(
                              (candidate) =>
                                includedRefs.has(candidate.finding_ref) &&
                                candidate.finding_ref !== finding.finding_ref,
                            )
                            .map((candidate) => candidate.id);
                          const ok = await setFindings(remaining);
                          if (ok) toast.push(`${finding.finding_ref} removed from this report`);
                        }}
                      >
                        Remove
                      </Button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {editable && (
            <div className="row">
              <Button onClick={() => setShowPicker(true)}>Choose findings</Button>
              <Button variant="ghost" onClick={() => navigate("/findings?new=1")}>
                Create a new finding
              </Button>
            </div>
          )}
        </>
      )}

      {showPicker && (
        <PickerList
          title="Findings"
          onClose={() => setShowPicker(false)}
          loading={all.loading}
          items={(all.data ?? []).map((finding) => ({
            id: finding.id,
            primary: finding.title,
            secondary: `${finding.finding_ref} · ${finding.affected_asset || "no asset recorded"}`,
            badge: (
              <span className="row">
                <SeverityBadge severity={finding.severity} />
                {!finding.severity_confirmed && <SeverityConfirmationBadge confirmed={false} />}
              </span>
            ),
          }))}
          selected={includedIds}
          onSave={async (ids) => {
            const ok = await setFindings(ids);
            if (ok) {
              toast.success("Findings updated");
              setShowPicker(false);
            }
          }}
          busy={state.busy}
        />
      )}
    </div>
  );
}

export function EvidencePicker({
  workspace,
  editable,
  onChanged,
}: {
  workspace: ReportWorkspace;
  editable: boolean;
  onChanged: () => Promise<void>;
}) {
  const toast = useToast();
  const [showPicker, setShowPicker] = useState(false);
  const all = useAsync(() => api.listEvidence({ limit: 500 }), [showPicker]);

  const includedRefs = useMemo(
    () => new Set(workspace.version.content.evidence.map((item) => item.evidence_ref)),
    [workspace.version.content.evidence],
  );
  const includedIds = useMemo(
    () => (all.data ?? []).filter((item) => includedRefs.has(item.evidence_ref)).map((item) => item.id),
    [all.data, includedRefs],
  );

  const [setEvidence, state] = useAction(async (ids: string[]) => {
    await api.saveReportDraft(workspace.report.id, { evidence_ids: ids });
    await onChanged();
    return true;
  });

  return (
    <div className="stack">
      <p className="small muted">
        Evidence is cited in the appendix with its filename and SHA-256 digest, so a reader can
        verify the copy they hold. Images are embedded; other formats are referenced.
      </p>

      {state.error && <Notice tone="danger">{state.error.message}</Notice>}

      {workspace.version.content.evidence.length === 0 ? (
        <EmptyState
          title="No evidence attached"
          hint="Import files on the Evidence screen, then attach them here."
          action={
            editable ? (
              <Button variant="primary" onClick={() => setShowPicker(true)}>
                Choose evidence
              </Button>
            ) : undefined
          }
        />
      ) : (
        <>
          <table className="table">
            <thead>
              <tr>
                <th>Reference</th>
                <th>Title</th>
                <th>File</th>
                <th>Size</th>
                <th>SHA-256</th>
              </tr>
            </thead>
            <tbody>
              {workspace.version.content.evidence.map((item) => (
                <tr key={item.evidence_ref}>
                  <td className="table__ref">{item.evidence_ref}</td>
                  <td className="table__main table__truncate">{item.title}</td>
                  <td className="muted table__truncate">{item.original_filename}</td>
                  <td className="muted nowrap">{formatBytes(item.byte_size)}</td>
                  <td className="mono table__truncate">{item.sha256.slice(0, 16)}…</td>
                </tr>
              ))}
            </tbody>
          </table>
          {editable && <Button onClick={() => setShowPicker(true)}>Choose evidence</Button>}
        </>
      )}

      {showPicker && (
        <PickerList
          title="Evidence"
          onClose={() => setShowPicker(false)}
          loading={all.loading}
          items={(all.data ?? []).map((item) => ({
            id: item.id,
            primary: item.title,
            secondary: `${item.evidence_ref} · ${item.original_filename} · ${formatBytes(item.byte_size)}`,
            badge: item.is_encrypted ? <span className="badge badge--accent">Encrypted</span> : null,
          }))}
          selected={includedIds}
          onSave={async (ids) => {
            const ok = await setEvidence(ids);
            if (ok) {
              toast.success("Evidence updated");
              setShowPicker(false);
            }
          }}
          busy={state.busy}
        />
      )}
    </div>
  );
}

// ---------------------------------------------------------------- shared picker

interface PickerItem {
  id: string;
  primary: string;
  secondary: string;
  badge?: React.ReactNode;
}

function PickerList({
  title,
  items,
  selected,
  onSave,
  onClose,
  loading,
  busy,
}: {
  title: string;
  items: PickerItem[];
  selected: string[];
  onSave: (ids: string[]) => Promise<void>;
  onClose: () => void;
  loading: boolean;
  busy: boolean;
}) {
  const [chosen, setChosen] = useState<string[]>(selected);
  const [filter, setFilter] = useState("");

  const visible = items.filter((item) => {
    const needle = filter.trim().toLowerCase();
    if (!needle) return true;
    return (
      item.primary.toLowerCase().includes(needle) || item.secondary.toLowerCase().includes(needle)
    );
  });

  return (
    <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <div className="modal modal--wide" role="dialog" aria-modal="true" aria-label={`Choose ${title}`}>
        <header className="modal__header">
          <h2 className="modal__title">Choose {title.toLowerCase()}</h2>
          <button type="button" className="btn btn--ghost btn--sm" style={{ marginLeft: "auto" }} onClick={onClose}>
            ✕
          </button>
        </header>
        <div className="modal__body">
          <input
            className="input"
            placeholder={`Filter ${title.toLowerCase()}`}
            value={filter}
            onChange={(event) => setFilter(event.target.value)}
            style={{ marginBottom: 12 }}
            aria-label={`Filter ${title}`}
            autoFocus
          />
          {loading ? (
            <LoadingBlock />
          ) : visible.length === 0 ? (
            <EmptyState title={`No ${title.toLowerCase()} available`} />
          ) : (
            <div className="stack stack--sm">
              {visible.map((item) => (
                <label key={item.id} className="checkbox card" style={{ padding: "9px 11px" }}>
                  <input
                    type="checkbox"
                    checked={chosen.includes(item.id)}
                    onChange={(event) =>
                      setChosen((current) =>
                        event.target.checked
                          ? [...current, item.id]
                          : current.filter((id) => id !== item.id),
                      )
                    }
                  />
                  <span className="checkbox__text" style={{ minWidth: 0, flex: 1 }}>
                    <span className="row row--between">
                      <span className="strong table__truncate">{item.primary}</span>
                      {item.badge}
                    </span>
                    <span className="checkbox__hint">{item.secondary}</span>
                  </span>
                </label>
              ))}
            </div>
          )}
        </div>
        <footer className="modal__footer">
          <span className="small muted grow">{chosen.length} selected</span>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" busy={busy} onClick={() => void onSave(chosen)}>
            Update report
          </Button>
        </footer>
      </div>
    </div>
  );
}
