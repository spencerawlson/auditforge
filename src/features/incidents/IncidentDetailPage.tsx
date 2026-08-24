/** Incident record editor, with the same explicit severity decision findings use. */

import { useEffect, useState } from "react";
import { useParams } from "react-router-dom";

import {
  SEVERITY_LABELS,
  SEVERITY_ORDER,
  SeverityBadge,
  SeverityConfirmationBadge,
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
import { formatDateTime, humanize } from "@/services/format";
import type { Incident, IncidentStatus, IncidentUpdate, Severity } from "@/types";

const NARRATIVE: Array<{ key: keyof IncidentUpdate & string; label: string; help: string }> = [
  { key: "description", label: "What happened?", help: "Plain, factual account." },
  { key: "impact", label: "Impact", help: "Operational, business and regulatory consequences." },
  { key: "timeline", label: "Timeline", help: "Timestamped sequence, with time zones." },
  { key: "response_actions", label: "Response actions", help: "Containment, eradication, recovery." },
  {
    key: "root_cause",
    label: "Root cause",
    help: "Leave blank if not established. A hypothesis is not a root cause.",
  },
  { key: "remediation", label: "Remediation", help: "What has been or will be fixed." },
  { key: "lessons_learned", label: "Lessons learned", help: "What to change as a result." },
];

const STATUS_OPTIONS: Array<{ value: IncidentStatus; label: string }> = [
  { value: "open", label: "Open" },
  { value: "investigating", label: "Investigating" },
  { value: "contained", label: "Contained" },
  { value: "resolved", label: "Resolved" },
  { value: "closed", label: "Closed" },
];

export function IncidentDetailPage() {
  const { id = "" } = useParams();
  const toast = useToast();
  const incident = useAsync(() => api.getIncident(id), [id]);
  const [draft, setDraft] = useState<IncidentUpdate>({});
  const [severityDialog, setSeverityDialog] = useState(false);

  useEffect(() => {
    if (!incident.data) return;
    const record = incident.data;
    setDraft({
      title: record.title,
      reporter: record.reporter,
      affected_systems: record.affected_systems,
      description: record.description,
      impact: record.impact,
      timeline: record.timeline,
      detection_method: record.detection_method,
      response_actions: record.response_actions,
      root_cause: record.root_cause,
      remediation: record.remediation,
      lessons_learned: record.lessons_learned,
      status: record.status,
      detected_at: record.detected_at,
      resolved_at: record.resolved_at,
    });
  }, [incident.data]);

  const [save, saveState] = useAction(async () => {
    const updated = await api.updateIncident(id, draft);
    incident.set(updated);
    return updated;
  });

  if (incident.loading && !incident.data) return <LoadingBlock />;
  if (incident.error) return <Notice tone="danger">{incident.error.message}</Notice>;
  if (!incident.data) return null;
  const record = incident.data;

  function update<K extends keyof IncidentUpdate>(key: K, value: IncidentUpdate[K]) {
    setDraft((current) => ({ ...current, [key]: value }));
  }

  return (
    <div className="page stack">
      <PageHeader
        title={record.title}
        subtitle={
          <span className="row row--wrap">
            <span className="mono">{record.incident_ref}</span>
            <SeverityBadge severity={record.severity} />
            <SeverityConfirmationBadge confirmed={record.severity_confirmed} />
            <span className="badge badge--outline">{humanize(record.status)}</span>
          </span>
        }
        actions={
          <>
            <Button onClick={() => setSeverityDialog(true)}>
              {record.severity_confirmed ? "Change severity" : "Set severity"}
            </Button>
            <Button
              variant="primary"
              busy={saveState.busy}
              onClick={() => void save().then(() => toast.success("Incident saved"))}
            >
              Save changes
            </Button>
          </>
        }
      />

      {saveState.error && <Notice tone="danger">{saveState.error.message}</Notice>}

      <div className="grid grid--2">
        <Card title="Record">
          <div className="stack">
            <TextField
              label="Title"
              value={draft.title ?? ""}
              onValueChange={(value) => update("title", value)}
              required
            />
            <TextField
              label="Reported by"
              value={draft.reporter ?? ""}
              onValueChange={(value) => update("reporter", value)}
            />
            <TextField
              label="Affected systems"
              value={draft.affected_systems ?? ""}
              onValueChange={(value) => update("affected_systems", value)}
            />
            <TextField
              label="Detection method"
              value={draft.detection_method ?? ""}
              onValueChange={(value) => update("detection_method", value)}
            />
            <SelectField
              label="Status"
              value={draft.status ?? "open"}
              options={STATUS_OPTIONS}
              onValueChange={(value) => update("status", value)}
            />
          </div>
        </Card>

        <Card title="Times">
          <div className="stack">
            <TextField
              label="Detected at"
              type="datetime-local"
              value={toLocalInput(draft.detected_at ?? null)}
              onValueChange={(value) => update("detected_at", fromLocalInput(value))}
              help="Recorded in UTC."
            />
            <TextField
              label="Resolved at"
              type="datetime-local"
              value={toLocalInput(draft.resolved_at ?? null)}
              onValueChange={(value) => update("resolved_at", fromLocalInput(value))}
              help="Leave empty while the incident is open."
            />
            <KeyValues
              items={[
                ["Recorded", formatDateTime(record.created_at)],
                ["Updated", formatDateTime(record.updated_at)],
              ]}
            />
          </div>
        </Card>
      </div>

      {NARRATIVE.map((field) => {
        const value = (draft[field.key] as string | undefined) ?? "";
        return (
          <Card
            key={field.key}
            title={field.label}
            actions={
              <AiAssist
                text={value}
                context={`Incident ${record.incident_ref}: ${record.title}. Field: ${field.label}.`}
                targetType="incident"
                targetId={record.id}
                onApply={(next) => update(field.key, next as never)}
              />
            }
          >
            <TextAreaField
              label={field.label}
              help={field.help}
              value={value}
              onValueChange={(next) => update(field.key, next as never)}
            />
          </Card>
        );
      })}

      {severityDialog && (
        <IncidentSeverityDialog
          incident={record}
          onClose={() => setSeverityDialog(false)}
          onConfirmed={(updated) => {
            incident.set(updated);
            setSeverityDialog(false);
            toast.success(`Severity confirmed as ${SEVERITY_LABELS[updated.severity]}`);
          }}
        />
      )}
    </div>
  );
}

/** `datetime-local` wants local wall-clock; the backend stores RFC 3339 UTC. */
function toLocalInput(value: string | null): string {
  if (!value) return "";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  const offset = date.getTimezoneOffset() * 60_000;
  return new Date(date.getTime() - offset).toISOString().slice(0, 16);
}

function fromLocalInput(value: string): string | null {
  if (!value) return null;
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return null;
  return `${date.toISOString().slice(0, 19)}Z`;
}

function IncidentSeverityDialog({
  incident,
  onClose,
  onConfirmed,
}: {
  incident: Incident;
  onClose: () => void;
  onConfirmed: (updated: Incident) => void;
}) {
  const [severity, setSeverity] = useState<Severity>(incident.severity);
  const [rationale, setRationale] = useState("");
  const [confirmed, setConfirmed] = useState(false);

  const [apply, { busy, error }] = useAction(async () =>
    api.setIncidentSeverity(incident.id, { severity, confirmed_by_user: confirmed, rationale }),
  );

  return (
    <Modal
      title="Confirm incident severity"
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
        <TextAreaField label="Rationale" value={rationale} onValueChange={setRationale} />
        <label className="checkbox">
          <input
            type="checkbox"
            checked={confirmed}
            onChange={(event) => setConfirmed(event.target.checked)}
          />
          <span className="checkbox__text">I am setting this severity as the responder.</span>
        </label>
      </div>
    </Modal>
  );
}
