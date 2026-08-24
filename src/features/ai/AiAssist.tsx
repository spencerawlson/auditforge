/**
 * The AI review interface.
 *
 * Two rules shape this component:
 *
 * 1. Nothing is applied without an explicit decision. The suggestion arrives, the author
 *    sees the original beside it and the changes between them, and only Accept or Save
 *    edit writes anything.
 * 2. The author knows where their text is going before it goes. The privacy banner and,
 *    for cloud calls, the exact list of values redaction will replace are shown up front.
 */

import { useState } from "react";

import { ConfidenceBadge, PrivacyBadge, ProvenanceBadge, PROVENANCE_HINTS } from "@/components/badges";
import { CompareView, DiffView } from "@/components/DiffView";
import { Modal } from "@/components/Modal";
import { useToast } from "@/components/Toast";
import { Button, LoadingBlock, Notice } from "@/components/ui";
import { useAction, useAsync } from "@/hooks/useAsync";
import { api } from "@/services/api";
import type { AiFunction, AiSuggestion, Provenance, SummaryKind } from "@/types";

/** The functions offered on a passage of prose, in the order they are most often wanted. */
const TEXT_FUNCTIONS: Array<{ id: AiFunction; label: string; blurb: string }> = [
  {
    id: "improve_writing",
    label: "Improve writing",
    blurb: "Grammar, spelling, clarity and structure. Facts are left exactly as written.",
  },
  {
    id: "rewrite",
    label: "Rewrite",
    blurb: "Rework the passage while preserving its factual meaning.",
  },
  {
    id: "technical_rewrite",
    label: "Technical rewrite",
    blurb: "Turn informal notes into professional technical prose.",
  },
  {
    id: "executive_rewrite",
    label: "Executive rewrite",
    blurb: "Restate for a non-technical decision maker, without adding a risk rating.",
  },
  {
    id: "summarize",
    label: "Summarize",
    blurb: "Condense to the essentials, using only what the passage contains.",
  },
  {
    id: "expand",
    label: "Expand",
    blurb:
      "Draft a fuller section from short notes. Adds no facts: gaps are marked “Information required”.",
  },
];

interface AiAssistProps {
  /** Current text of the field being worked on. */
  text: string;
  /** Short factual orientation, e.g. "Finding: TLS 1.0 on the VPN concentrator". */
  context?: string;
  targetType: string;
  targetId: string;
  disabled?: boolean;
  /** Called when the author accepts (or edits and saves) a suggestion. */
  onApply: (value: string, provenance: Provenance, requestId: string) => void;
}

export function AiAssist({
  text,
  context = "",
  targetType,
  targetId,
  disabled = false,
  onApply,
}: AiAssistProps) {
  const [open, setOpen] = useState(false);
  const status = useAsync(() => api.aiStatus(), [open]);

  if (!status.data?.enabled) {
    return (
      <Button
        size="sm"
        variant="ghost"
        disabled
        title="Turn on an AI provider in Settings > AI. AuditForge works fully without it."
      >
        AI assist (off)
      </Button>
    );
  }

  return (
    <>
      <Button size="sm" variant="ghost" disabled={disabled} onClick={() => setOpen(true)}>
        AI assist
      </Button>
      {open && (
        <AiAssistDialog
          text={text}
          context={context}
          targetType={targetType}
          targetId={targetId}
          onClose={() => setOpen(false)}
          onApply={(value, provenance, requestId) => {
            onApply(value, provenance, requestId);
            setOpen(false);
          }}
        />
      )}
    </>
  );
}

function AiAssistDialog({
  text,
  context,
  targetType,
  targetId,
  onClose,
  onApply,
}: Omit<AiAssistProps, "disabled"> & { onClose: () => void }) {
  const toast = useToast();
  const status = useAsync(() => api.aiStatus(), []);
  const [summaryKind, setSummaryKind] = useState<SummaryKind>("technical");
  const [suggestion, setSuggestion] = useState<AiSuggestion | null>(null);
  const [edited, setEdited] = useState<string | null>(null);

  const redaction = useAsync(
    () => (status.data?.redaction_enabled ? api.aiRedactionPreview(text) : Promise.resolve(null)),
    [status.data?.redaction_enabled, text],
  );

  const [run, runState] = useAction(async (fn: AiFunction) =>
    api.aiSuggest({
      function: fn,
      text,
      context,
      target_type: targetType,
      target_id: targetId,
      ...(fn === "summarize" ? { summary_kind: summaryKind } : {}),
    }),
  );

  const [decide, decideState] = useAction(
    async (requestId: string, decision: "accepted" | "edited" | "rejected") => {
      await api.aiRecordDecision(requestId, decision);
      return true;
    },
  );

  const tooShort = text.trim().length < 12;

  if (suggestion) {
    const value = edited ?? suggestion.suggestion;
    const wasEdited = edited !== null && edited !== suggestion.suggestion;

    return (
      <Modal
        title="Review the suggestion"
        onClose={onClose}
        wide
        footer={
          <>
            <Button
              variant="danger"
              busy={decideState.busy}
              onClick={async () => {
                await decide(suggestion.request_id, "rejected");
                toast.push("Suggestion rejected. Nothing was changed.");
                onClose();
              }}
            >
              Reject
            </Button>
            <div className="grow" />
            <Button onClick={() => setEdited(value)} disabled={edited !== null}>
              Edit before accepting
            </Button>
            <Button
              variant="primary"
              busy={decideState.busy}
              onClick={async () => {
                await decide(suggestion.request_id, wasEdited ? "edited" : "accepted");
                onApply(value, suggestion.provenance, suggestion.request_id);
                toast.success(wasEdited ? "Your edited version was applied." : "Suggestion applied.");
              }}
            >
              {wasEdited ? "Apply my edit" : "Accept"}
            </Button>
          </>
        }
      >
        <div className="stack">
          <div className="row row--wrap">
            <PrivacyBadge mode={suggestion.privacy_mode} />
            <ConfidenceBadge confidence={suggestion.confidence} />
            <span className="badge badge--outline">{suggestion.model || "model"}</span>
            <span className="tiny muted">
              If accepted, this text is recorded as: <ProvenanceBadge provenance={suggestion.provenance} />
            </span>
          </div>

          {suggestion.provenance === "ai_inference" && (
            <Notice tone="warning" title="This is assistant-drafted prose">
              {PROVENANCE_HINTS.ai_inference} Read it against your own notes before accepting.
            </Notice>
          )}

          {suggestion.missing_information.length > 0 && (
            <Notice tone="info" title="Information the assistant says is missing">
              <ul style={{ margin: "4px 0 0", paddingLeft: 18 }}>
                {suggestion.missing_information.map((item, index) => (
                  <li key={index}>{item}</li>
                ))}
              </ul>
            </Notice>
          )}

          {edited === null ? (
            <>
              <CompareView original={suggestion.original} suggestion={suggestion.suggestion} />
              <div>
                <div className="compare__label">Changes</div>
                <div className="compare__text">
                  <DiffView diff={suggestion.diff} />
                </div>
              </div>
            </>
          ) : (
            <div>
              <div className="compare__label">Your version</div>
              <textarea
                className="textarea textarea--tall"
                value={edited}
                onChange={(event) => setEdited(event.target.value)}
                autoFocus
              />
              <p className="tiny muted" style={{ marginTop: 6 }}>
                Editing before accepting is recorded as an edited suggestion in the audit trail.
              </p>
            </div>
          )}

          {suggestion.notes.length > 0 && (
            <div className="tiny muted">
              {suggestion.notes.map((note, index) => (
                <div key={index}>· {note}</div>
              ))}
            </div>
          )}
        </div>
      </Modal>
    );
  }

  return (
    <Modal
      title="AI assist"
      onClose={onClose}
      wide
      footer={<Button onClick={onClose}>Cancel</Button>}
    >
      <div className="stack">
        {status.data && (
          <div className="row row--wrap">
            <PrivacyBadge mode={status.data.privacy_mode} title={status.data.destination} />
            <span className="tiny muted">{status.data.destination}</span>
          </div>
        )}

        {status.data?.leaves_machine && (
          <Notice tone="warning" title="This text will leave your machine">
            The passage below is sent to OpenAI. Do not use cloud processing for material you are
            not permitted to share with a third party.
          </Notice>
        )}

        {status.data?.redaction_enabled && redaction.data && redaction.data.detections.length > 0 && (
          <Notice tone="info" title={`${redaction.data.detections.length} value(s) will be redacted before sending`}>
            <div className="tiny mono" style={{ marginTop: 4 }}>
              {redaction.data.detections.map((detection, index) => (
                <div key={index}>
                  {detection.kind}: {detection.text}
                </div>
              ))}
            </div>
            <p className="tiny" style={{ marginTop: 6, marginBottom: 0 }}>
              Real values are restored in the suggestion you see. Redaction covers structured
              values only, not hostnames or names.
            </p>
          </Notice>
        )}

        {tooShort && (
          <Notice tone="neutral">
            There is not enough text here yet. Write a sentence or two first; the assistant works
            from what you have written, not from nothing.
          </Notice>
        )}

        {runState.error && <Notice tone="danger">{runState.error.message}</Notice>}
        {runState.busy && <LoadingBlock label="Working" />}

        {!runState.busy && (
          <div className="stack stack--sm">
            {TEXT_FUNCTIONS.map((fn) => (
              <div key={fn.id} className="card" style={{ padding: "10px 12px" }}>
                <div className="row row--between">
                  <div style={{ minWidth: 0 }}>
                    <div className="strong small">{fn.label}</div>
                    <div className="tiny muted">{fn.blurb}</div>
                  </div>
                  <Button
                    size="sm"
                    disabled={tooShort}
                    onClick={async () => {
                      const result = await run(fn.id);
                      if (result) setSuggestion(result);
                    }}
                  >
                    Run
                  </Button>
                </div>
                {fn.id === "summarize" && (
                  <div className="row" style={{ marginTop: 8 }}>
                    <span className="tiny muted">Audience:</span>
                    <select
                      className="select"
                      style={{ maxWidth: 190, height: 26, padding: "0 8px" }}
                      value={summaryKind}
                      onChange={(event) => setSummaryKind(event.target.value as SummaryKind)}
                      aria-label="Summary audience"
                    >
                      <option value="executive">Executive summary</option>
                      <option value="technical">Technical summary</option>
                      <option value="incident">Incident summary</option>
                      <option value="finding">Finding summary</option>
                    </select>
                  </div>
                )}
              </div>
            ))}
          </div>
        )}

        <p className="tiny faint">
          The assistant helps with language. It does not decide severity, invent identifiers, or
          add facts your text does not already contain; suggestions that do are refused before you
          see them.
        </p>
      </div>
    </Modal>
  );
}
