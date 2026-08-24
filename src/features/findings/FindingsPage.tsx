/** Finding list, filters, and the create dialog. */

import { useEffect, useState } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";

import {
  SEVERITY_ORDER,
  SeverityBadge,
  SeverityConfirmationBadge,
  VerificationBadge,
} from "@/components/badges";
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
import { formatRelative } from "@/services/format";
import type { NewFinding, Severity } from "@/types";

export function FindingsPage() {
  const navigate = useNavigate();
  const [params, setParams] = useSearchParams();
  const [search, setSearch] = useState("");
  const [severity, setSeverity] = useState<Severity | "all">("all");
  const [creating, setCreating] = useState(params.get("new") === "1");
  const query = useDebouncedValue(search);

  useEffect(() => {
    if (params.get("new") === "1") {
      setCreating(true);
      params.delete("new");
      setParams(params, { replace: true });
    }
  }, [params, setParams]);

  const findings = useAsync(
    () =>
      api.listFindings({
        ...(query.trim() ? { search: query.trim() } : {}),
        ...(severity === "all" ? {} : { severity }),
      }),
    [query, severity],
  );

  const unconfirmed = findings.data?.filter((finding) => !finding.severity_confirmed).length ?? 0;

  return (
    <div className="page stack">
      <PageHeader
        title="Findings"
        subtitle="Findings are recorded once and can be included in any number of reports."
        actions={
          <Button variant="primary" onClick={() => setCreating(true)}>
            New finding
          </Button>
        }
      />

      {unconfirmed > 0 && (
        <Notice tone="warning" title="Severity decisions outstanding">
          {unconfirmed} finding{unconfirmed === 1 ? "" : "s"} below have no confirmed severity.
          AuditForge never assigns severity on your behalf, and a report cannot be finalized while
          it includes one.
        </Notice>
      )}

      <div className="row">
        <input
          className="input"
          style={{ maxWidth: 340 }}
          placeholder="Filter by title, reference, asset or category"
          value={search}
          onChange={(event) => setSearch(event.target.value)}
          aria-label="Filter findings"
        />
        <select
          className="select"
          style={{ maxWidth: 180 }}
          value={severity}
          onChange={(event) => setSeverity(event.target.value as Severity | "all")}
          aria-label="Filter by severity"
        >
          <option value="all">All severities</option>
          {SEVERITY_ORDER.map((value) => (
            <option key={value} value={value}>
              {value.charAt(0).toUpperCase() + value.slice(1)}
            </option>
          ))}
        </select>
      </div>

      {findings.error && <Notice tone="danger">{findings.error.message}</Notice>}

      <Card flush>
        {findings.loading && !findings.data ? (
          <LoadingBlock />
        ) : !findings.data || findings.data.length === 0 ? (
          <EmptyState
            title="No findings"
            hint="Record what you found once; reports reference it, and a finalized report keeps its own copy."
            action={
              <Button variant="primary" onClick={() => setCreating(true)}>
                Create a finding
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
                <th>Affected asset</th>
                <th>Status</th>
                <th className="right">Updated</th>
              </tr>
            </thead>
            <tbody>
              {findings.data.map((finding) => (
                <tr
                  key={finding.id}
                  data-clickable="true"
                  onClick={() => navigate(`/findings/${finding.id}`)}
                >
                  <td className="nowrap">
                    <SeverityBadge severity={finding.severity} />
                    {!finding.severity_confirmed && (
                      <>
                        {" "}
                        <SeverityConfirmationBadge confirmed={false} />
                      </>
                    )}
                  </td>
                  <td className="table__ref">{finding.finding_ref}</td>
                  <td className="table__main table__truncate">{finding.title}</td>
                  <td className="muted table__truncate">{finding.affected_asset || "—"}</td>
                  <td>
                    <VerificationBadge status={finding.verification_status} />
                  </td>
                  <td className="table__num muted nowrap tiny">
                    {formatRelative(finding.updated_at)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Card>

      {creating && (
        <CreateFindingDialog
          onClose={() => setCreating(false)}
          onCreated={(id) => navigate(`/findings/${id}`)}
        />
      )}
    </div>
  );
}

function CreateFindingDialog({
  onClose,
  onCreated,
}: {
  onClose: () => void;
  onCreated: (id: string) => void;
}) {
  const toast = useToast();
  const engagements = useAsync(() => api.listEngagements({ limit: 200 }), []);
  const [form, setForm] = useState<NewFinding>({
    title: "",
    affected_asset: "",
    description: "",
    engagement_id: null,
  });

  const [create, { busy, error }] = useAction(async () => api.createFinding(form));

  function update<K extends keyof NewFinding>(key: K, value: NewFinding[K]) {
    setForm((current) => ({ ...current, [key]: value }));
  }

  return (
    <Modal
      title="New finding"
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
                toast.success(`${created.finding_ref} created`);
                onCreated(created.id);
              }
            }}
          >
            Create finding
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
          placeholder="TLS 1.0 enabled on the VPN concentrator"
          required
          autoFocus
        />
        <TextField
          label="Affected asset"
          value={form.affected_asset ?? ""}
          onValueChange={(value) => update("affected_asset", value)}
          placeholder="vpn.example.internal"
        />
        <TextAreaField
          label="Description"
          value={form.description ?? ""}
          onValueChange={(value) => update("description", value)}
          help="What you observed. Write it plainly; you can improve the wording later."
        />

        <div className="field">
          <label className="field__label" htmlFor="finding-engagement">
            Engagement
          </label>
          <select
            id="finding-engagement"
            className="select"
            value={form.engagement_id ?? ""}
            onChange={(event) => update("engagement_id", event.target.value || null)}
          >
            <option value="">Not assigned</option>
            {engagements.data?.map((engagement) => (
              <option key={engagement.id} value={engagement.id}>
                {engagement.engagement_ref} — {engagement.name}
              </option>
            ))}
          </select>
        </div>

        <Notice tone="info" title="Severity is set separately">
          A new finding starts as Informational and unconfirmed. You set and confirm the severity
          on the finding itself, so that decision is always deliberate and always recorded.
        </Notice>
      </div>
    </Modal>
  );
}
