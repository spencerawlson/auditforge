/** Engagement detail: the record itself, plus what has been produced inside it. */

import { useState } from "react";
import { useNavigate, useParams } from "react-router-dom";

import { ReportStatusBadge, SeverityBadge } from "@/components/badges";
import { Modal } from "@/components/Modal";
import { useToast } from "@/components/Toast";
import {
  Button,
  Card,
  EmptyState,
  KeyValues,
  LoadingBlock,
  Notice,
  PageHeader,
  Tabs,
  TextAreaField,
  TextField,
} from "@/components/ui";
import { useAction, useAsync } from "@/hooks/useAsync";
import { api } from "@/services/api";
import { formatBytes, formatDate, formatRelative } from "@/services/format";
import type { EngagementStatus, EngagementUpdate } from "@/types";

import { StatusSelect } from "./EngagementsPage";

type Tab = "overview" | "reports" | "findings" | "evidence";

export function EngagementDetailPage() {
  const { id = "" } = useParams();
  const navigate = useNavigate();
  const toast = useToast();
  const [tab, setTab] = useState<Tab>("overview");
  const [editing, setEditing] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);

  const engagement = useAsync(() => api.getEngagement(id), [id]);
  const reports = useAsync(() => api.listReports({ engagement_id: id }), [id]);
  const findings = useAsync(() => api.listFindings({ engagement_id: id }), [id]);
  const evidence = useAsync(() => api.listEvidence({ engagement_id: id }), [id]);

  const [setStatus, statusState] = useAction(async (status: EngagementStatus) => {
    const updated = await api.updateEngagement(id, { status });
    engagement.set(updated);
    return updated;
  });

  const [remove, removeState] = useAction(async () => {
    await api.deleteEngagement(id);
    return true;
  });

  if (engagement.loading && !engagement.data) return <LoadingBlock />;
  if (engagement.error) return <Notice tone="danger">{engagement.error.message}</Notice>;
  if (!engagement.data) return null;
  const record = engagement.data;

  return (
    <div className="page stack">
      <PageHeader
        title={record.name}
        subtitle={
          <span className="row">
            <span className="mono">{record.engagement_ref}</span>
            {record.client_name && <span>· {record.client_name}</span>}
            <span>· updated {formatRelative(record.updated_at)}</span>
          </span>
        }
        actions={
          <>
            <Button onClick={() => setEditing(true)}>Edit</Button>
            <Button
              variant="primary"
              onClick={() => navigate(`/reports?new=1&engagement=${record.id}`)}
            >
              New report
            </Button>
          </>
        }
      />

      <div className="row" style={{ maxWidth: 260 }}>
        <StatusSelect
          value={record.status}
          busy={statusState.busy}
          onChange={(status) => void setStatus(status)}
        />
      </div>
      {statusState.error && <Notice tone="danger">{statusState.error.message}</Notice>}

      <Tabs
        active={tab}
        onChange={setTab}
        tabs={[
          { id: "overview", label: "Overview" },
          { id: "reports", label: `Reports (${reports.data?.length ?? 0})` },
          { id: "findings", label: `Findings (${findings.data?.length ?? 0})` },
          { id: "evidence", label: `Evidence (${evidence.data?.length ?? 0})` },
        ]}
      />

      {tab === "overview" && (
        <div className="stack">
          <Card title="Engagement details">
            <KeyValues
              items={[
                ["Reference", <span className="mono">{record.engagement_ref}</span>],
                ["Client", record.client_name || "—"],
                [
                  "Period",
                  record.start_date
                    ? `${formatDate(record.start_date)}${record.end_date ? ` → ${formatDate(record.end_date)}` : ""}`
                    : "—",
                ],
                ["Authorization", record.authorization_ref || "Not recorded"],
                [
                  "Team",
                  record.team.length > 0
                    ? record.team
                        .map((member) => (member.role ? `${member.name} (${member.role})` : member.name))
                        .join(", ")
                    : "—",
                ],
              ]}
            />
          </Card>

          {!record.authorization_ref && (
            <Notice tone="warning" title="No authorization recorded">
              Record the signed rules of engagement, contract or ticket that authorises this work.
              Reports generated from this engagement will otherwise have nothing to cite.
            </Notice>
          )}

          <div className="grid grid--2">
            <TextBlock title="Objectives" text={record.objectives} />
            <TextBlock title="Scope" text={record.scope} />
            <TextBlock title="Out of scope" text={record.out_of_scope} />
            <TextBlock title="Rules of engagement" text={record.rules_of_engagement} />
          </div>

          {record.notes && <TextBlock title="Notes" text={record.notes} />}

          <Card title="Danger zone">
            <div className="row row--between">
              <span className="small muted">
                Deleting an engagement removes it and its team list. Reports and findings are kept
                and become unassigned. Engagements with finalized reports cannot be deleted.
              </span>
              <Button variant="danger" onClick={() => setConfirmDelete(true)}>
                Delete engagement
              </Button>
            </div>
          </Card>
        </div>
      )}

      {tab === "reports" && (
        <Card flush>
          {(reports.data?.length ?? 0) === 0 ? (
            <EmptyState title="No reports in this engagement" />
          ) : (
            <table className="table">
              <thead>
                <tr>
                  <th>Reference</th>
                  <th>Title</th>
                  <th>Status</th>
                  <th>Version</th>
                  <th className="right">Updated</th>
                </tr>
              </thead>
              <tbody>
                {reports.data?.map((report) => (
                  <tr key={report.id} data-clickable="true" onClick={() => navigate(`/reports/${report.id}`)}>
                    <td className="table__ref">{report.report_ref}</td>
                    <td className="table__main table__truncate">{report.title}</td>
                    <td>
                      <ReportStatusBadge status={report.status} />
                    </td>
                    <td className="muted nowrap">{report.current_version_label ?? "—"}</td>
                    <td className="table__num muted nowrap tiny">{formatRelative(report.updated_at)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </Card>
      )}

      {tab === "findings" && (
        <Card flush>
          {(findings.data?.length ?? 0) === 0 ? (
            <EmptyState title="No findings in this engagement" />
          ) : (
            <table className="table">
              <thead>
                <tr>
                  <th>Severity</th>
                  <th>Reference</th>
                  <th>Title</th>
                  <th>Affected asset</th>
                  <th className="right">Updated</th>
                </tr>
              </thead>
              <tbody>
                {findings.data?.map((finding) => (
                  <tr key={finding.id} data-clickable="true" onClick={() => navigate(`/findings/${finding.id}`)}>
                    <td>
                      <SeverityBadge severity={finding.severity} />
                    </td>
                    <td className="table__ref">{finding.finding_ref}</td>
                    <td className="table__main table__truncate">{finding.title}</td>
                    <td className="muted table__truncate">{finding.affected_asset || "—"}</td>
                    <td className="table__num muted nowrap tiny">{formatRelative(finding.updated_at)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </Card>
      )}

      {tab === "evidence" && (
        <Card flush>
          {(evidence.data?.length ?? 0) === 0 ? (
            <EmptyState title="No evidence in this engagement" />
          ) : (
            <table className="table">
              <thead>
                <tr>
                  <th>Reference</th>
                  <th>Title</th>
                  <th>File</th>
                  <th>Size</th>
                  <th className="right">Added</th>
                </tr>
              </thead>
              <tbody>
                {evidence.data?.map((item) => (
                  <tr key={item.id}>
                    <td className="table__ref">{item.evidence_ref}</td>
                    <td className="table__main table__truncate">{item.title}</td>
                    <td className="muted table__truncate">{item.original_filename}</td>
                    <td className="muted nowrap">{formatBytes(item.byte_size)}</td>
                    <td className="table__num muted nowrap tiny">{formatRelative(item.created_at)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </Card>
      )}

      {editing && (
        <EditEngagementDialog
          initial={record}
          onClose={() => setEditing(false)}
          onSaved={(updated) => {
            engagement.set(updated);
            setEditing(false);
            toast.success("Engagement updated");
          }}
        />
      )}

      {confirmDelete && (
        <Modal
          title="Delete this engagement?"
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
                    toast.success(`${record.engagement_ref} deleted`);
                    navigate("/engagements");
                  }
                }}
              >
                Delete
              </Button>
            </>
          }
        >
          <div className="stack">
            <p>
              <span className="strong">{record.name}</span> will be removed. Reports, findings and
              evidence created inside it are kept and become unassigned.
            </p>
            {removeState.error && <Notice tone="danger">{removeState.error.message}</Notice>}
          </div>
        </Modal>
      )}
    </div>
  );
}

function TextBlock({ title, text }: { title: string; text: string }) {
  return (
    <Card title={title}>
      {text.trim() ? (
        <p className="prewrap small">{text}</p>
      ) : (
        <p className="faint small">Not recorded.</p>
      )}
    </Card>
  );
}

function EditEngagementDialog({
  initial,
  onClose,
  onSaved,
}: {
  initial: import("@/types").Engagement;
  onClose: () => void;
  onSaved: (updated: import("@/types").Engagement) => void;
}) {
  const [form, setForm] = useState<EngagementUpdate>({
    name: initial.name,
    client_name: initial.client_name,
    start_date: initial.start_date ?? "",
    end_date: initial.end_date ?? "",
    objectives: initial.objectives,
    scope: initial.scope,
    out_of_scope: initial.out_of_scope,
    authorization_ref: initial.authorization_ref,
    rules_of_engagement: initial.rules_of_engagement,
    notes: initial.notes,
  });

  const [save, { busy, error }] = useAction(async () =>
    api.updateEngagement(initial.id, {
      ...form,
      start_date: form.start_date ? form.start_date : null,
      end_date: form.end_date ? form.end_date : null,
    }),
  );

  function update<K extends keyof EngagementUpdate>(key: K, value: EngagementUpdate[K]) {
    setForm((current) => ({ ...current, [key]: value }));
  }

  return (
    <Modal
      title="Edit engagement"
      onClose={onClose}
      wide
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button
            variant="primary"
            busy={busy}
            onClick={async () => {
              const updated = await save();
              if (updated) onSaved(updated);
            }}
          >
            Save changes
          </Button>
        </>
      }
    >
      <div className="stack">
        {error && <Notice tone="danger">{error.message}</Notice>}
        <TextField
          label="Engagement name"
          value={form.name ?? ""}
          onValueChange={(value) => update("name", value)}
          required
        />
        <TextField
          label="Client or company"
          value={form.client_name ?? ""}
          onValueChange={(value) => update("client_name", value)}
        />
        <div className="grid grid--2">
          <TextField
            label="Start date"
            type="date"
            value={form.start_date ?? ""}
            onValueChange={(value) => update("start_date", value)}
          />
          <TextField
            label="End date"
            type="date"
            value={form.end_date ?? ""}
            onValueChange={(value) => update("end_date", value)}
          />
        </div>
        <TextField
          label="Authorization reference"
          value={form.authorization_ref ?? ""}
          onValueChange={(value) => update("authorization_ref", value)}
        />
        <TextAreaField
          label="Objectives"
          value={form.objectives ?? ""}
          onValueChange={(value) => update("objectives", value)}
        />
        <TextAreaField
          label="Scope"
          value={form.scope ?? ""}
          onValueChange={(value) => update("scope", value)}
        />
        <TextAreaField
          label="Out of scope"
          value={form.out_of_scope ?? ""}
          onValueChange={(value) => update("out_of_scope", value)}
        />
        <TextAreaField
          label="Rules of engagement"
          value={form.rules_of_engagement ?? ""}
          onValueChange={(value) => update("rules_of_engagement", value)}
        />
        <TextAreaField
          label="Notes"
          value={form.notes ?? ""}
          onValueChange={(value) => update("notes", value)}
        />
      </div>
    </Modal>
  );
}
