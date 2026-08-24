/**
 * The report editor's right-hand column: readiness and finalization, the AI quality
 * review, preview and export, and version history.
 */

import { useState } from "react";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import { ConfidenceBadge, PrivacyBadge, ReportStatusBadge } from "@/components/badges";
import { Modal } from "@/components/Modal";
import { useToast } from "@/components/Toast";
import { Button, Card, LoadingBlock, Notice } from "@/components/ui";
import { useAction, useAsync } from "@/hooks/useAsync";
import { api } from "@/services/api";
import { formatDateTime, humanize } from "@/services/format";
import type { AiReviewResult, ExportFormat, ReportWorkspace } from "@/types";

// ---------------------------------------------------------------- readiness

export function FinalizePanel({
  workspace,
  onChanged,
  dirty,
}: {
  workspace: ReportWorkspace;
  onChanged: () => Promise<void>;
  dirty: boolean;
}) {
  const toast = useToast();
  const [confirming, setConfirming] = useState(false);

  const blockers = workspace.readiness.issues.filter((issue) => issue.severity === "blocker");
  const warnings = workspace.readiness.issues.filter((issue) => issue.severity === "warning");
  const isFinal = workspace.version.status === "final";

  const [submit, submitState] = useAction(async () => {
    await api.submitReportForReview(workspace.report.id);
    await onChanged();
    return true;
  });

  const [revise, reviseState] = useAction(async () => {
    await api.createReportRevision(workspace.report.id);
    await onChanged();
    return true;
  });

  return (
    <Card title="Readiness">
      <div className="stack stack--sm">
        {isFinal ? (
          <>
            <Notice tone="success" title="Finalized">
              {workspace.version.version_label}, sealed{" "}
              {formatDateTime(workspace.version.finalized_at)}.
            </Notice>
            <Button
              busy={reviseState.busy}
              onClick={async () => {
                const ok = await revise();
                if (ok) toast.success("New revision opened. The issued version is unchanged.");
              }}
            >
              Create new revision
            </Button>
            {reviseState.error && <Notice tone="danger">{reviseState.error.message}</Notice>}
          </>
        ) : (
          <>
            {blockers.length === 0 ? (
              <Notice tone="success">This report is ready to finalize.</Notice>
            ) : (
              <Notice tone="warning" title={`${blockers.length} thing(s) to resolve first`}>
                <ul style={{ margin: "4px 0 0", paddingLeft: 18 }}>
                  {blockers.slice(0, 6).map((issue, index) => (
                    <li key={index}>{issue.message}</li>
                  ))}
                  {blockers.length > 6 && <li>and {blockers.length - 6} more…</li>}
                </ul>
              </Notice>
            )}

            {warnings.length > 0 && (
              <details>
                <summary className="small muted" style={{ cursor: "pointer" }}>
                  {warnings.length} warning{warnings.length === 1 ? "" : "s"} (do not block)
                </summary>
                <ul className="tiny muted" style={{ margin: "6px 0 0", paddingLeft: 18 }}>
                  {warnings.map((issue, index) => (
                    <li key={index}>{issue.message}</li>
                  ))}
                </ul>
              </details>
            )}

            {workspace.version.status !== "review" && (
              <Button
                busy={submitState.busy}
                onClick={async () => {
                  const ok = await submit();
                  if (ok) toast.success("Moved to review");
                }}
              >
                Submit for review
              </Button>
            )}

            <Button
              variant="primary"
              disabled={blockers.length > 0 || dirty}
              title={dirty ? "Save your changes first" : undefined}
              onClick={() => setConfirming(true)}
            >
              Finalize report
            </Button>
            {dirty && <p className="tiny muted">Save your draft before finalizing.</p>}
            {submitState.error && <Notice tone="danger">{submitState.error.message}</Notice>}
          </>
        )}
      </div>

      {confirming && (
        <FinalizeDialog
          workspace={workspace}
          warnings={warnings.length}
          onClose={() => setConfirming(false)}
          onFinalized={async () => {
            setConfirming(false);
            await onChanged();
            toast.success("Report finalized. This version is now immutable.");
          }}
        />
      )}
    </Card>
  );
}

function FinalizeDialog({
  workspace,
  warnings,
  onClose,
  onFinalized,
}: {
  workspace: ReportWorkspace;
  warnings: number;
  onClose: () => void;
  onFinalized: () => Promise<void>;
}) {
  const [notes, setNotes] = useState("");
  const [understood, setUnderstood] = useState(false);
  const [finalize, { busy, error }] = useAction(async () =>
    api.finalizeReport(workspace.report.id, true, notes),
  );

  return (
    <Modal
      title="Finalize this report?"
      onClose={onClose}
      dismissible={false}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button
            variant="primary"
            busy={busy}
            disabled={!understood}
            onClick={async () => {
              const done = await finalize();
              if (done) await onFinalized();
            }}
          >
            Finalize
          </Button>
        </>
      }
    >
      <div className="stack">
        {error && <Notice tone="danger">{error.message}</Notice>}

        <p>
          Finalizing seals <span className="strong">{workspace.report.report_ref}</span> as a new
          immutable version. It cannot be edited afterwards; corrections are made by creating a
          revision, which leaves the issued document untouched.
        </p>

        {warnings > 0 && (
          <Notice tone="warning">
            {warnings} warning{warnings === 1 ? "" : "s"} remain. They do not block finalization,
            but they are worth reading first.
          </Notice>
        )}

        <div className="field">
          <label className="field__label" htmlFor="finalize-notes">
            Notes for the version history
          </label>
          <input
            id="finalize-notes"
            className="input"
            value={notes}
            onChange={(event) => setNotes(event.target.value)}
            placeholder="Approved by the author; issued to the client"
          />
        </div>

        <label className="checkbox">
          <input
            type="checkbox"
            checked={understood}
            onChange={(event) => setUnderstood(event.target.checked)}
          />
          <span className="checkbox__text">
            I have reviewed this report and take responsibility for its contents.
            <span className="checkbox__hint">
              The facts, findings, severities, conclusions and remediation are mine as the author.
            </span>
          </span>
        </label>
      </div>
    </Modal>
  );
}

// ---------------------------------------------------------------- AI review

export function AiReviewPanel({
  reportId,
  onJumpToSection,
}: {
  reportId: string;
  onJumpToSection: (key: string) => void;
}) {
  const status = useAsync(() => api.aiStatus(), []);
  const [result, setResult] = useState<AiReviewResult | null>(null);
  const [showSent, setShowSent] = useState(false);

  const [review, { busy, error }] = useAction(async () => api.aiReviewReport(reportId));
  const sent = useAsync(
    () => (showSent ? api.aiReviewPreview(reportId) : Promise.resolve("")),
    [showSent, reportId],
  );

  return (
    <Card title="Quality review">
      <div className="stack stack--sm">
        {status.data && <PrivacyBadge mode={status.data.privacy_mode} title={status.data.destination} />}

        <p className="tiny muted">
          Checks the report for contradictions, unsupported claims, missing information,
          inconsistent terminology and timeline problems. It returns warnings only and never
          edits anything.
        </p>

        {!status.data?.enabled ? (
          <Notice tone="neutral">
            AI is turned off. The readiness checks above run locally and are unaffected.
          </Notice>
        ) : (
          <>
            <Button
              busy={busy}
              onClick={async () => {
                const outcome = await review();
                if (outcome) setResult(outcome);
              }}
            >
              Run review
            </Button>
            <button
              type="button"
              className="btn btn--ghost btn--sm"
              onClick={() => setShowSent((value) => !value)}
            >
              {showSent ? "Hide" : "Show"} what would be sent
            </button>
            {showSent && (
              <pre className="mono" style={{ maxHeight: 200, overflow: "auto", whiteSpace: "pre-wrap" }}>
                {sent.loading ? "…" : sent.data}
              </pre>
            )}
          </>
        )}

        {error && <Notice tone="danger">{error.message}</Notice>}

        {result && (
          <div className="stack stack--sm">
            <div className="row row--wrap">
              <ConfidenceBadge confidence={result.confidence} />
              <span className="badge badge--outline">{result.model}</span>
            </div>
            {result.summary && <p className="small">{result.summary}</p>}
            {result.warnings.length === 0 ? (
              <Notice tone="success">The reviewer raised nothing.</Notice>
            ) : (
              result.warnings.map((warning, index) => (
                <div key={index} className="notice notice--warning" style={{ display: "block" }}>
                  <div className="tiny strong">{humanize(warning.category)}</div>
                  <div className="small">{warning.message}</div>
                  {warning.quote && (
                    <div className="tiny muted" style={{ marginTop: 4, fontStyle: "italic" }}>
                      “{warning.quote}”
                    </div>
                  )}
                  {warning.section_key && (
                    <button
                      type="button"
                      className="btn btn--ghost btn--sm"
                      style={{ marginTop: 4 }}
                      onClick={() => onJumpToSection(warning.section_key!)}
                    >
                      Go to section
                    </button>
                  )}
                </div>
              ))
            )}
            <p className="tiny faint">
              These are prompts to look again, not corrections. Nothing has been changed.
            </p>
          </div>
        )}
      </div>
    </Card>
  );
}

// ---------------------------------------------------------------- preview and export

const FORMATS: Array<{ id: ExportFormat; label: string; blurb: string }> = [
  { id: "pdf", label: "PDF", blurb: "The finished document." },
  { id: "html", label: "HTML", blurb: "One self-contained page." },
  { id: "markdown", label: "Markdown", blurb: "For a wiki or a ticket." },
  { id: "json", label: "JSON", blurb: "The stored document, for archival." },
  { id: "csv", label: "Findings CSV", blurb: "The findings table only." },
];

export function ExportPanel({ workspace }: { workspace: ReportWorkspace }) {
  const toast = useToast();
  const [preview, setPreview] = useState<string | null>(null);

  const [renderPreview, previewState] = useAction(async () => {
    const rendered = await api.previewReportPdf(workspace.report.id);
    return `data:application/pdf;base64,${rendered.base64}`;
  });

  const [exportAs, exportState] = useAction(async (format: ExportFormat) => {
    const suggested = await api.exportFilename(workspace.report.id, format);
    const destination = await saveDialog({ defaultPath: suggested });
    if (!destination) return null;
    return api.exportReportTo(workspace.report.id, format, destination);
  });

  return (
    <Card title="Preview and export">
      <div className="stack stack--sm">
        <Button
          busy={previewState.busy}
          onClick={async () => {
            const url = await renderPreview();
            if (url) setPreview(url);
          }}
        >
          Preview PDF
        </Button>

        {workspace.version.status !== "final" && (
          <p className="tiny muted">
            You are previewing {workspace.version.version_label}. Every page carries the version
            label, so a draft can never be mistaken for an issued report.
          </p>
        )}

        <div className="stack stack--sm">
          {FORMATS.map((format) => (
            <div key={format.id} className="row row--between">
              <span className="tiny">
                <span className="strong">{format.label}</span> <span className="muted">— {format.blurb}</span>
              </span>
              <Button
                size="sm"
                variant="ghost"
                busy={exportState.busy}
                onClick={async () => {
                  const path = await exportAs(format.id);
                  if (path) toast.success(`Exported to ${path}`);
                }}
              >
                Export
              </Button>
            </div>
          ))}
        </div>

        {(previewState.error || exportState.error) && (
          <Notice tone="danger">
            {(previewState.error ?? exportState.error)?.message}
          </Notice>
        )}
      </div>

      {preview && (
        <Modal
          title={`${workspace.report.report_ref} — ${workspace.version.version_label}`}
          onClose={() => setPreview(null)}
          wide
          footer={
            <>
              <Button
                onClick={async () => {
                  const path = await exportAs("pdf");
                  if (path) {
                    toast.success(`Saved to ${path}`);
                    try {
                      await openPath(path);
                    } catch {
                      // Opening is a convenience; the file is saved either way.
                    }
                  }
                }}
              >
                Save PDF
              </Button>
              <Button variant="primary" onClick={() => setPreview(null)}>
                Close
              </Button>
            </>
          }
        >
          <iframe className="pdf-frame" src={preview} title="Report preview" />
        </Modal>
      )}
    </Card>
  );
}

// ---------------------------------------------------------------- history

export function VersionHistory({ workspace }: { workspace: ReportWorkspace }) {
  return (
    <Card title="Version history">
      {workspace.history.length === 0 ? (
        <LoadingBlock />
      ) : (
        <div className="stack stack--sm">
          {workspace.history.map((version) => (
            <div key={version.id} className="row row--between">
              <div style={{ minWidth: 0 }}>
                <div className="small strong">
                  {version.version_label}{" "}
                  {version.id === workspace.version.id && (
                    <span className="badge badge--accent">Current</span>
                  )}
                </div>
                <div className="tiny muted">
                  {formatDateTime(version.created_at)} · {version.author_name}
                </div>
                {version.notes && <div className="tiny faint">{version.notes}</div>}
              </div>
              <ReportStatusBadge status={version.status} />
            </div>
          ))}
          <p className="tiny faint">
            Each entry records a SHA-256 of its content, so a version can be checked against a copy
            issued earlier.
          </p>
        </div>
      )}
    </Card>
  );
}
