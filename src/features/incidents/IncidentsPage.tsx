/** Incident list and creation. */

import { useEffect, useState } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";

import { SeverityBadge, SeverityConfirmationBadge } from "@/components/badges";
import { Modal } from "@/components/Modal";
import { useToast } from "@/components/Toast";
import {
  Button,
  Card,
  EmptyState,
  LoadingBlock,
  Notice,
  PageHeader,
  TextAreaField,
  TextField,
} from "@/components/ui";
import { useAction, useAsync, useDebouncedValue } from "@/hooks/useAsync";
import { api } from "@/services/api";
import { formatDateTime, humanize } from "@/services/format";
import type { IncidentStatus, NewIncident } from "@/types";

export function IncidentsPage() {
  const navigate = useNavigate();
  const [params, setParams] = useSearchParams();
  const [search, setSearch] = useState("");
  const [status, setStatus] = useState<IncidentStatus | "all">("all");
  const [creating, setCreating] = useState(params.get("new") === "1");
  const query = useDebouncedValue(search);

  useEffect(() => {
    if (params.get("new") === "1") {
      setCreating(true);
      params.delete("new");
      setParams(params, { replace: true });
    }
  }, [params, setParams]);

  const incidents = useAsync(
    () =>
      api.listIncidents({
        ...(query.trim() ? { search: query.trim() } : {}),
        ...(status === "all" ? {} : { status }),
      }),
    [query, status],
  );

  return (
    <div className="page stack">
      <PageHeader
        title="Incidents"
        subtitle="A factual record of what happened, what it affected, and how it was handled."
        actions={
          <Button variant="primary" onClick={() => setCreating(true)}>
            New incident
          </Button>
        }
      />

      <div className="row">
        <input
          className="input"
          style={{ maxWidth: 340 }}
          placeholder="Filter by title, reference or affected system"
          value={search}
          onChange={(event) => setSearch(event.target.value)}
          aria-label="Filter incidents"
        />
        <select
          className="select"
          style={{ maxWidth: 190 }}
          value={status}
          onChange={(event) => setStatus(event.target.value as IncidentStatus | "all")}
          aria-label="Filter by status"
        >
          <option value="all">All statuses</option>
          {(["open", "investigating", "contained", "resolved", "closed"] as IncidentStatus[]).map(
            (value) => (
              <option key={value} value={value}>
                {humanize(value)}
              </option>
            ),
          )}
        </select>
      </div>

      {incidents.error && <Notice tone="danger">{incidents.error.message}</Notice>}

      <Card flush>
        {incidents.loading && !incidents.data ? (
          <LoadingBlock />
        ) : !incidents.data || incidents.data.length === 0 ? (
          <EmptyState
            title="No incidents recorded"
            hint="Record incidents as they happen; a postmortem can be attached to one later."
            action={
              <Button variant="primary" onClick={() => setCreating(true)}>
                Record an incident
              </Button>
            }
          />
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>Severity</th>
                <th>Reference</th>
                <th>Title</th>
                <th>Affected systems</th>
                <th>Status</th>
                <th className="right">Detected</th>
              </tr>
            </thead>
            <tbody>
              {incidents.data.map((incident) => (
                <tr
                  key={incident.id}
                  data-clickable="true"
                  onClick={() => navigate(`/incidents/${incident.id}`)}
                >
                  <td className="nowrap">
                    <SeverityBadge severity={incident.severity} />
                    {!incident.severity_confirmed && (
                      <>
                        {" "}
                        <SeverityConfirmationBadge confirmed={false} />
                      </>
                    )}
                  </td>
                  <td className="table__ref">{incident.incident_ref}</td>
                  <td className="table__main table__truncate">{incident.title}</td>
                  <td className="muted table__truncate">{incident.affected_systems || "—"}</td>
                  <td className="muted nowrap">{humanize(incident.status)}</td>
                  <td className="table__num muted nowrap tiny">
                    {formatDateTime(incident.detected_at)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Card>

      {creating && (
        <CreateIncidentDialog
          onClose={() => setCreating(false)}
          onCreated={(id) => navigate(`/incidents/${id}`)}
        />
      )}
    </div>
  );
}

function CreateIncidentDialog({
  onClose,
  onCreated,
}: {
  onClose: () => void;
  onCreated: (id: string) => void;
}) {
  const toast = useToast();
  const [form, setForm] = useState<NewIncident>({
    title: "",
    reporter: "",
    affected_systems: "",
    description: "",
    detection_method: "",
    detected_at: new Date().toISOString().slice(0, 19) + "Z",
  });

  const [create, { busy, error }] = useAction(async () => api.createIncident(form));

  function update<K extends keyof NewIncident>(key: K, value: NewIncident[K]) {
    setForm((current) => ({ ...current, [key]: value }));
  }

  return (
    <Modal
      title="Record an incident"
      onClose={onClose}
      wide
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button
            variant="primary"
            busy={busy}
            onClick={async () => {
              const created = await create();
              if (created) {
                toast.success(`${created.incident_ref} recorded`);
                onCreated(created.id);
              }
            }}
          >
            Record incident
          </Button>
        </>
      }
    >
      <div className="stack">
        {error && <Notice tone="danger">{error.message}</Notice>}
        <TextField
          label="Title"
          value={form.title}
          onValueChange={(value) => update("title", value)}
          placeholder="Unauthorized access to the jump host"
          required
          autoFocus
        />
        <TextField
          label="Reported by"
          value={form.reporter ?? ""}
          onValueChange={(value) => update("reporter", value)}
          placeholder="SOC"
        />
        <TextField
          label="Affected systems"
          value={form.affected_systems ?? ""}
          onValueChange={(value) => update("affected_systems", value)}
          placeholder="jump01, bastion02"
        />
        <TextField
          label="How was it detected?"
          value={form.detection_method ?? ""}
          onValueChange={(value) => update("detection_method", value)}
          placeholder="EDR alert"
        />
        <TextAreaField
          label="What happened?"
          value={form.description ?? ""}
          onValueChange={(value) => update("description", value)}
          help="Facts only. Anything not yet established can be left out and added later."
        />
        <Notice tone="info">
          Severity starts unconfirmed. You set it on the incident itself, so the decision is
          deliberate and recorded.
        </Notice>
      </div>
    </Modal>
  );
}
