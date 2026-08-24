/**
 * Finding editor.
 *
 * Narrative fields save together; severity does not. Confirming a severity is its own
 * dialog and its own backend command, because it is the one judgement the product refuses
 * to make for the author.
 */

import { useEffect, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";

import {
  SEVERITY_LABELS,
  SEVERITY_ORDER,
  SeverityBadge,
  SeverityConfirmationBadge,
  ProvenanceBadge,
  VerificationBadge,
} from "@/components/badges";
import { Modal } from "@/components/Modal";
import { useToast } from "@/components/Toast";
import {
  Button,
  Card,
  KeyValues,
  LoadingBlock,
  Notice,
  PageHeader,
  SelectField,
  TextAreaField,
  TextField,
} from "@/components/ui";
import { AiAssist } from "@/features/ai/AiAssist";
import { useAction, useAsync } from "@/hooks/useAsync";
import { api } from "@/services/api";
import { formatDateTime } from "@/services/format";
import type { Finding, FindingUpdate, Provenance, Severity, VerificationStatus } from "@/types";

/** The narrative fields, in the order they read in a report. */
const NARRATIVE: Array<{ key: keyof FindingUpdate & string; label: string; help: string }> = [
  { key: "description", label: "Description", help: "What was found, in plain language." },
  {
    key: "technical_details",
    label: "Technical details",
    help: "Commands, requests, responses or configuration that show the issue.",
  },
  { key: "impact", label: "Impact", help: "What an attacker or a failure could achieve." },
  { key: "likelihood", label: "Likelihood", help: "How readily this could be exploited or occur." },
  { key: "risk", label: "Risk", help: "Your assessment of the combined risk." },
  { key: "root_cause", label: "Root cause", help: "Why it exists. Leave blank if not established." },
  { key: "remediation", label: "Remediation", help: "The specific fix." },
  { key: "recommendation", label: "Recommendation", help: "Wider improvement beyond the fix." },
];

const VERIFICATION_OPTIONS: Array<{ value: VerificationStatus; label: string }> = [
  { value: "unverified", label: "Unverified" },
  { value: "verified", label: "Verified" },
  { value: "remediated", label: "Remediated" },
  { value: "risk_accepted", label: "Risk accepted" },
  { value: "false_positive", label: "False positive" },
];

export function FindingDetailPage() {
  const { id = "" } = useParams();
  const navigate = useNavigate();
  const toast = useToast();
  const finding = useAsync(() => api.getFinding(id), [id]);

  const [draft, setDraft] = useState<FindingUpdate>({});
  const [provenance, setProvenance] = useState<Record<string, Provenance>>({});
  const [severityDialog, setSeverityDialog] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);

  useEffect(() => {
    if (!finding.data) return;
    const record = finding.data;
    setDraft({
      title: record.title,
      affected_asset: record.affected_asset,
      category: record.category,
      description: record.description,
      technical_details: record.technical_details,
      impact: record.impact,
      likelihood: record.likelihood,
      risk: record.risk,
      root_cause: record.root_cause,
      remediation: record.remediation,
      recommendation: record.recommendation,
      verification_status: record.verification_status,
      reference_links: record.reference_links,
      cvss_vector: record.cvss_vector,
      cvss_score: record.cvss_score,
    });
    setProvenance({});
  }, [finding.data]);

  const [save, saveState] = useAction(async () => {
    const updated = await api.updateFinding(id, {
      ...draft,
      ...(Object.keys(provenance).length > 0 ? { provenance } : {}),
    });
    finding.set(updated);
    return updated;
  });

  const [remove, removeState] = useAction(async () => {
    await api.deleteFinding(id);
    return true;
  });

  if (finding.loading && !finding.data) return <LoadingBlock />;
  if (finding.error) return <Notice tone="danger">{finding.error.message}</Notice>;
  if (!finding.data) return null;
  const record = finding.data;

  function update<K extends keyof FindingUpdate>(key: K, value: FindingUpdate[K]) {
    setDraft((current) => ({ ...current, [key]: value }));
  }

  function applySuggestion(field: string, value: string, kind: Provenance) {
    setDraft((current) => ({ ...current, [field]: value }));
    setProvenance((current) => ({ ...current, [field]: kind }));
  }

  return (
    <div className="page stack">
      <PageHeader
        title={record.title}
        subtitle={
          <span className="row row--wrap">
            <span className="mono">{record.finding_ref}</span>
            <SeverityBadge severity={record.severity} />
            <SeverityConfirmationBadge confirmed={record.severity_confirmed} />
            <VerificationBadge status={record.verification_status} />
          </span>
        }
        actions={
          <>
            <Button onClick={() => setSeverityDialog(true)}>
              {record.severity_confirmed ? "Change severity" : "Set severity"}
            </Button>
            <Button variant="primary" busy={saveState.busy} onClick={() => void save().then(() => toast.success("Finding saved"))}>
              Save changes
            </Button>
          </>
        }
      />

      {!record.severity_confirmed && (
        <Notice tone="warning" title="Severity has not been confirmed">
          This finding is recorded as {SEVERITY_LABELS[record.severity]}, but no one has confirmed
          that. Any report including it cannot be finalized until you do.
        </Notice>
      )}

      {saveState.error && <Notice tone="danger">{saveState.error.message}</Notice>}

      <div className="grid grid--2">
        <Card title="Identification">
          <div className="stack">
            <TextField
              label="Title"
              value={draft.title ?? ""}
              onValueChange={(value) => update("title", value)}
              required
            />
            <TextField
              label="Affected asset"
              value={draft.affected_asset ?? ""}
              onValueChange={(value) => update("affected_asset", value)}
              placeholder="vpn.example.internal"
            />
            <TextField
              label="Category"
              value={draft.category ?? ""}
              onValueChange={(value) => update("category", value)}
              placeholder="Configuration"
            />
            <SelectField
              label="Verification status"
              value={draft.verification_status ?? "unverified"}
              options={VERIFICATION_OPTIONS}
              onValueChange={(value) => update("verification_status", value)}
            />
          </div>
        </Card>

        <Card title="Scoring and references">
          <div className="stack">
            <TextField
              label="CVSS score"
              type="number"
              min={0}
              max={10}
              step={0.1}
              value={draft.cvss_score ?? ""}
              onValueChange={(value) => update("cvss_score", value === "" ? null : Number(value))}
              help="Optional. Between 0.0 and 10.0."
            />
            <TextField
              label="CVSS vector"
              value={draft.cvss_vector ?? ""}
              onValueChange={(value) => update("cvss_vector", value)}
              placeholder="CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H"
            />
            <TextAreaField
              label="References"
              value={draft.reference_links ?? ""}
              onValueChange={(value) => update("reference_links", value)}
              help="One per line."
            />
          </div>
        </Card>
      </div>

      {NARRATIVE.map((field) => {
        const value = (draft[field.key] as string | undefined) ?? "";
        const current = provenance[field.key] ?? record.provenance[field.key] ?? "user_provided";
        return (
          <Card
            key={field.key}
            title={
              <span className="row">
                {field.label}
                <ProvenanceBadge provenance={current} />
              </span>
            }
            actions={
              <AiAssist
                text={value}
                context={`Finding ${record.finding_ref}: ${record.title}. Field: ${field.label}.`}
                targetType="finding"
                targetId={record.id}
                onApply={(next, kind) => applySuggestion(field.key, next, kind)}
              />
            }
          >
            <TextAreaField
              label={field.label}
              help={field.help}
              value={value}
              onValueChange={(next) => {
                update(field.key, next as never);
                // Typing over assisted text makes it the author's own words again.
                setProvenance((currentMap) => ({ ...currentMap, [field.key]: "user_provided" }));
              }}
            />
          </Card>
        );
      })}

      <Card title="Record">
        <KeyValues
          items={[
            ["Reference", <span className="mono">{record.finding_ref}</span>],
            ["Author", record.author_name],
            ["Created", formatDateTime(record.created_at)],
            ["Updated", formatDateTime(record.updated_at)],
            ["Evidence", record.evidence_ids.length > 0 ? `${record.evidence_ids.length} item(s)` : "None attached"],
          ]}
        />
        <div className="row row--between" style={{ marginTop: 16 }}>
          <span className="small muted">
            Deleting a finding does not alter reports that have already been finalized: those keep
            their own copy.
          </span>
          <Button variant="danger" onClick={() => setConfirmDelete(true)}>
            Delete finding
          </Button>
        </div>
      </Card>

      {severityDialog && (
        <SeverityDialog
          finding={record}
          onClose={() => setSeverityDialog(false)}
          onConfirmed={(updated) => {
            finding.set(updated);
            setSeverityDialog(false);
            toast.success(`Severity confirmed as ${SEVERITY_LABELS[updated.severity]}`);
          }}
        />
      )}

      {confirmDelete && (
        <Modal
          title="Delete this finding?"
          onClose={() => setConfirmDelete(false)}
          dismissible={false}
          footer={
            <>
              <Button onClick={() => setConfirmDelete(false)}>Cancel</Button>
              <Button
                variant="danger"
                busy={removeState.busy}
                onClick={async () => {
                  const done = await remove();
                  if (done) {
                    toast.success(`${record.finding_ref} deleted`);
                    navigate("/findings");
                  }
                }}
              >
                Delete
              </Button>
            </>
          }
        >
          <p>
            <span className="strong">{record.finding_ref}</span> will be removed from the finding
            library.
          </p>
          {removeState.error && <Notice tone="danger">{removeState.error.message}</Notice>}
        </Modal>
      )}
    </div>
  );
}

/**
 * The severity decision.
 *
 * The confirmation checkbox is not a formality: the backend rejects the call without it,
 * so this dialog is the only route by which a severity can be set.
 */
function SeverityDialog({
  finding,
  onClose,
  onConfirmed,
}: {
  finding: Finding;
  onClose: () => void;
  onConfirmed: (updated: Finding) => void;
}) {
  const [severity, setSeverity] = useState<Severity>(finding.severity);
  const [rationale, setRationale] = useState("");
  const [confirmed, setConfirmed] = useState(false);

  const [apply, { busy, error }] = useAction(async () =>
    api.setFindingSeverity(finding.id, {
      severity,
      confirmed_by_user: confirmed,
      rationale,
    }),
  );

  return (
    <Modal
      title="Confirm severity"
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button
            variant="primary"
            busy={busy}
            disabled={!confirmed}
            onClick={async () => {
              const updated = await apply();
              if (updated) onConfirmed(updated);
            }}
          >
            Confirm severity
          </Button>
        </>
      }
    >
      <div className="stack">
        {error && <Notice tone="danger">{error.message}</Notice>}

        <div className="field">
          <span className="field__label">Severity</span>
          <div className="row row--wrap" role="radiogroup" aria-label="Severity">
            {SEVERITY_ORDER.map((value) => (
              <button
                key={value}
                type="button"
                role="radio"
                aria-checked={severity === value}
                className={`btn${severity === value ? " btn--primary" : ""}`}
                onClick={() => setSeverity(value)}
              >
                {SEVERITY_LABELS[value]}
              </button>
            ))}
          </div>
        </div>

        <TextAreaField
          label="Rationale"
          value={rationale}
          onValueChange={setRationale}
          help="Recorded in the audit trail alongside the change. Optional, but worth writing: it is what you will read when someone challenges the rating."
        />

        <label className="checkbox">
          <input
            type="checkbox"
            checked={confirmed}
            onChange={(event) => setConfirmed(event.target.checked)}
          />
          <span className="checkbox__text">
            I am setting this severity as the author.
            <span className="checkbox__hint">
              AuditForge never assigns severity itself, and no AI suggestion can set it.
            </span>
          </span>
        </label>
      </div>
    </Modal>
  );
}
