/** Engagement list, with the create dialog. */

import { useEffect, useState } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";

import { EngagementStatusBadge } from "@/components/badges";
import { Modal } from "@/components/Modal";
import { useToast } from "@/components/Toast";
import {
  Button,
  Card,
  EmptyState,
  LoadingBlock,
  Notice,
  PageHeader,
  SelectField,
  TextAreaField,
  TextField,
} from "@/components/ui";
import { useAction, useAsync, useDebouncedValue } from "@/hooks/useAsync";
import { api } from "@/services/api";
import { formatDate, formatRelative } from "@/services/format";
import type { EngagementStatus, NewEngagement } from "@/types";

const STATUS_OPTIONS: Array<{ value: EngagementStatus | "all"; label: string }> = [
  { value: "all", label: "All statuses" },
  { value: "draft", label: "Draft" },
  { value: "active", label: "Active" },
  { value: "review", label: "Review" },
  { value: "completed", label: "Completed" },
  { value: "archived", label: "Archived" },
];

export function EngagementsPage() {
  const navigate = useNavigate();
  const [params, setParams] = useSearchParams();
  const [search, setSearch] = useState("");
  const [status, setStatus] = useState<EngagementStatus | "all">("all");
  const [creating, setCreating] = useState(params.get("new") === "1");
  const query = useDebouncedValue(search);

  useEffect(() => {
    if (params.get("new") === "1") {
      setCreating(true);
      params.delete("new");
      setParams(params, { replace: true });
    }
  }, [params, setParams]);

  const engagements = useAsync(
    () =>
      api.listEngagements({
        ...(query.trim() ? { search: query.trim() } : {}),
        ...(status === "all" ? {} : { status }),
      }),
    [query, status],
  );

  return (
    <div className="page stack">
      <PageHeader
        title="Engagements"
        subtitle="The container for a piece of work: scope, authorization, team, and everything produced."
        actions={
          <Button variant="primary" onClick={() => setCreating(true)}>
            New engagement
          </Button>
        }
      />

      <div className="row">
        <input
          className="input"
          style={{ maxWidth: 320 }}
          placeholder="Filter by name, client or reference"
          value={search}
          onChange={(event) => setSearch(event.target.value)}
          aria-label="Filter engagements"
        />
        <select
          className="select"
          style={{ maxWidth: 180 }}
          value={status}
          onChange={(event) => setStatus(event.target.value as EngagementStatus | "all")}
          aria-label="Filter by status"
        >
          {STATUS_OPTIONS.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
      </div>

      {engagements.error && <Notice tone="danger">{engagements.error.message}</Notice>}

      <Card flush>
        {engagements.loading && !engagements.data ? (
          <LoadingBlock />
        ) : !engagements.data || engagements.data.length === 0 ? (
          <EmptyState
            title="No engagements"
            hint="An engagement groups reports, findings and evidence under one client and scope."
            action={
              <Button variant="primary" onClick={() => setCreating(true)}>
                Create the first one
              </Button>
            }
          />
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>Reference</th>
                <th>Name</th>
                <th>Client</th>
                <th>Status</th>
                <th>Dates</th>
                <th className="right">Contents</th>
                <th className="right">Updated</th>
              </tr>
            </thead>
            <tbody>
              {engagements.data.map((engagement) => (
                <tr
                  key={engagement.id}
                  data-clickable="true"
                  onClick={() => navigate(`/engagements/${engagement.id}`)}
                >
                  <td className="table__ref">{engagement.engagement_ref}</td>
                  <td className="table__main table__truncate">{engagement.name}</td>
                  <td className="muted">{engagement.client_name || "—"}</td>
                  <td>
                    <EngagementStatusBadge status={engagement.status} />
                  </td>
                  <td className="muted nowrap tiny">
                    {engagement.start_date ? formatDate(engagement.start_date) : "—"}
                    {engagement.end_date ? ` → ${formatDate(engagement.end_date)}` : ""}
                  </td>
                  <td className="table__num muted nowrap tiny">
                    {engagement.report_count} rpt · {engagement.finding_count} fnd ·{" "}
                    {engagement.evidence_count} evd
                  </td>
                  <td className="table__num muted nowrap tiny">
                    {formatRelative(engagement.updated_at)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Card>

      {creating && (
        <CreateEngagementDialog
          onClose={() => setCreating(false)}
          onCreated={(id) => navigate(`/engagements/${id}`)}
        />
      )}
    </div>
  );
}

const EMPTY: NewEngagement = {
  name: "",
  client_name: "",
  start_date: "",
  end_date: "",
  objectives: "",
  scope: "",
  out_of_scope: "",
  authorization_ref: "",
  rules_of_engagement: "",
  notes: "",
};

function CreateEngagementDialog({
  onClose,
  onCreated,
}: {
  onClose: () => void;
  onCreated: (id: string) => void;
}) {
  const toast = useToast();
  const [form, setForm] = useState<NewEngagement>(EMPTY);
  const [create, { busy, error }] = useAction(async () => {
    const payload: NewEngagement = {
      ...form,
      start_date: form.start_date || null,
      end_date: form.end_date || null,
    };
    return api.createEngagement(payload);
  });

  function update<K extends keyof NewEngagement>(key: K, value: NewEngagement[K]) {
    setForm((current) => ({ ...current, [key]: value }));
  }

  return (
    <Modal
      title="New engagement"
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
                toast.success(`${created.engagement_ref} created`);
                onCreated(created.id);
              }
            }}
          >
            Create engagement
          </Button>
        </>
      }
    >
      <div className="stack">
        {error && <Notice tone="danger">{error.message}</Notice>}

        <TextField
          label="Engagement name"
          value={form.name}
          onValueChange={(value) => update("name", value)}
          placeholder="Q3 External Penetration Test"
          required
          autoFocus
        />
        <TextField
          label="Client or company"
          value={form.client_name ?? ""}
          onValueChange={(value) => update("client_name", value)}
          placeholder="Northwind Traders"
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
        <TextAreaField
          label="Objectives"
          value={form.objectives ?? ""}
          onValueChange={(value) => update("objectives", value)}
          help="What this work is meant to determine or demonstrate."
        />
        <TextAreaField
          label="Scope"
          value={form.scope ?? ""}
          onValueChange={(value) => update("scope", value)}
          help="Systems, networks, accounts or applications that are in scope."
        />
        <TextField
          label="Authorization reference"
          value={form.authorization_ref ?? ""}
          onValueChange={(value) => update("authorization_ref", value)}
          help="The signed rules of engagement, contract or change ticket that authorises this work."
          placeholder="Signed ROE 2026-08-20"
        />
      </div>
    </Modal>
  );
}

/** Exported for reuse by the engagement detail screen's status control. */
export function StatusSelect({
  value,
  onChange,
  busy,
}: {
  value: EngagementStatus;
  onChange: (status: EngagementStatus) => void;
  busy?: boolean;
}) {
  return (
    <SelectField
      label="Status"
      value={value}
      onValueChange={onChange}
      disabled={busy}
      options={STATUS_OPTIONS.filter((option) => option.value !== "all").map((option) => ({
        value: option.value as EngagementStatus,
        label: option.label,
      }))}
    />
  );
}
