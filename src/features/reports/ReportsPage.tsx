/** Report list and the "new report" flow, which is where a report type gets chosen. */

import { useEffect, useState } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";

import { ClassificationBadge, ReportStatusBadge } from "@/components/badges";
import { Modal } from "@/components/Modal";
import { useToast } from "@/components/Toast";
import {
  Button,
  Card,
  EmptyState,
  LoadingBlock,
  Notice,
  PageHeader,
  TextField,
} from "@/components/ui";
import { useAction, useAsync, useDebouncedValue } from "@/hooks/useAsync";
import { api } from "@/services/api";
import { formatRelative } from "@/services/format";
import type { Classification, ReportStatus, TemplateSummary } from "@/types";

const STATUS_OPTIONS: Array<{ value: ReportStatus | "all"; label: string }> = [
  { value: "all", label: "All statuses" },
  { value: "draft", label: "Draft" },
  { value: "review", label: "In review" },
  { value: "final", label: "Final" },
  { value: "archived", label: "Archived" },
];

export function ReportsPage() {
  const navigate = useNavigate();
  const [params, setParams] = useSearchParams();
  const [search, setSearch] = useState("");
  const [status, setStatus] = useState<ReportStatus | "all">("all");
  const [creating, setCreating] = useState(params.get("new") === "1");
  const [presetEngagement, setPresetEngagement] = useState<string | null>(params.get("engagement"));
  const query = useDebouncedValue(search);

  useEffect(() => {
    if (params.get("new") === "1") {
      setCreating(true);
      setPresetEngagement(params.get("engagement"));
      params.delete("new");
      params.delete("engagement");
      setParams(params, { replace: true });
    }
  }, [params, setParams]);

  const reports = useAsync(
    () =>
      api.listReports({
        ...(query.trim() ? { search: query.trim() } : {}),
        ...(status === "all" ? {} : { status }),
      }),
    [query, status],
  );

  return (
    <div className="page stack">
      <PageHeader
        title="Reports"
        subtitle="Answer the guided questions; AuditForge produces the document."
        actions={
          <Button variant="primary" onClick={() => setCreating(true)}>
            New report
          </Button>
        }
      />

      <div className="row">
        <input
          className="input"
          style={{ maxWidth: 340 }}
          placeholder="Filter by title or reference"
          value={search}
          onChange={(event) => setSearch(event.target.value)}
          aria-label="Filter reports"
        />
        <select
          className="select"
          style={{ maxWidth: 180 }}
          value={status}
          onChange={(event) => setStatus(event.target.value as ReportStatus | "all")}
          aria-label="Filter by status"
        >
          {STATUS_OPTIONS.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
      </div>

      {reports.error && <Notice tone="danger">{reports.error.message}</Notice>}

      <Card flush>
        {reports.loading && !reports.data ? (
          <LoadingBlock />
        ) : !reports.data || reports.data.length === 0 ? (
          <EmptyState
            title="No reports"
            hint="Pick a report type and AuditForge asks the questions that type needs."
            action={
              <Button variant="primary" onClick={() => setCreating(true)}>
                Create a report
              </Button>
            }
          />
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>Reference</th>
                <th>Title</th>
                <th>Engagement</th>
                <th>Status</th>
                <th>Version</th>
                <th>Classification</th>
                <th className="right">Findings</th>
                <th className="right">Updated</th>
              </tr>
            </thead>
            <tbody>
              {reports.data.map((report) => (
                <tr
                  key={report.id}
                  data-clickable="true"
                  onClick={() => navigate(`/reports/${report.id}`)}
                >
                  <td className="table__ref">{report.report_ref}</td>
                  <td className="table__main table__truncate">{report.title}</td>
                  <td className="muted table__truncate">{report.engagement_name ?? "—"}</td>
                  <td>
                    <ReportStatusBadge status={report.status} />
                  </td>
                  <td className="muted nowrap">{report.current_version_label ?? "—"}</td>
                  <td>
                    <ClassificationBadge value={report.classification} />
                  </td>
                  <td className="table__num muted">{report.finding_count}</td>
                  <td className="table__num muted nowrap tiny">
                    {formatRelative(report.updated_at)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Card>

      {creating && (
        <CreateReportDialog
          presetEngagement={presetEngagement}
          onClose={() => setCreating(false)}
          onCreated={(id) => navigate(`/reports/${id}`)}
        />
      )}
    </div>
  );
}

const CLASSIFICATIONS: Array<{ value: Classification; label: string; hint: string }> = [
  { value: "public", label: "Public", hint: "May be shared without restriction." },
  { value: "internal", label: "Internal", hint: "For the organisation only." },
  { value: "confidential", label: "Confidential", hint: "Named recipients only." },
  { value: "restricted", label: "Restricted", hint: "Tightly controlled distribution." },
];

function CreateReportDialog({
  presetEngagement,
  onClose,
  onCreated,
}: {
  presetEngagement: string | null;
  onClose: () => void;
  onCreated: (id: string) => void;
}) {
  const toast = useToast();
  const templates = useAsync(() => api.listTemplates(), []);
  const engagements = useAsync(() => api.listEngagements({ limit: 200 }), []);

  const [title, setTitle] = useState("");
  const [templateKey, setTemplateKey] = useState<string | null>(null);
  const [engagementId, setEngagementId] = useState<string | null>(presetEngagement);
  const [classification, setClassification] = useState<Classification | null>(null);

  const selected = templates.data?.find((template) => template.key === templateKey) ?? null;

  const [create, { busy, error }] = useAction(async () => {
    if (!templateKey) throw new Error("choose a report type");
    return api.createReport({
      title,
      template_key: templateKey,
      engagement_id: engagementId,
      ...(classification ? { classification } : {}),
    });
  });

  return (
    <Modal
      title="New report"
      onClose={onClose}
      wide
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button
            variant="primary"
            busy={busy}
            disabled={!templateKey || title.trim().length === 0}
            onClick={async () => {
              const created = await create();
              if (created) {
                toast.success(`${created.report_ref} created`);
                onCreated(created.id);
              }
            }}
          >
            Create report
          </Button>
        </>
      }
    >
      <div className="stack">
        {error && <Notice tone="danger">{error.message}</Notice>}

        <TextField
          label="Report title"
          value={title}
          onValueChange={setTitle}
          placeholder="Q3 External Penetration Test"
          required
          autoFocus
        />

        <div className="field">
          <span className="field__label field__label--required">Report type</span>
          <div className="field__help">
            The type decides which sections exist and which questions you are asked.
          </div>
          {templates.loading ? (
            <LoadingBlock />
          ) : (
            <div className="grid grid--2" style={{ gap: 8, marginTop: 6 }}>
              {templates.data?.map((template) => (
                <TemplateOption
                  key={template.key}
                  template={template}
                  selected={template.key === templateKey}
                  onSelect={() => {
                    setTemplateKey(template.key);
                    setClassification(template.default_classification);
                  }}
                />
              ))}
            </div>
          )}
        </div>

        <div className="field">
          <label className="field__label" htmlFor="report-engagement">
            Engagement
          </label>
          <div className="field__help">
            Optional. Linking an engagement copies its client, dates and team onto the report.
          </div>
          <select
            id="report-engagement"
            className="select"
            value={engagementId ?? ""}
            onChange={(event) => setEngagementId(event.target.value || null)}
          >
            <option value="">Not assigned</option>
            {engagements.data?.map((engagement) => (
              <option key={engagement.id} value={engagement.id}>
                {engagement.engagement_ref} — {engagement.name}
              </option>
            ))}
          </select>
        </div>

        {selected && (
          <div className="field">
            <span className="field__label">Classification</span>
            <div className="field__help">
              Printed in the header and footer of every page. Defaults to the type's usual level.
            </div>
            <div className="row row--wrap" style={{ marginTop: 6 }}>
              {CLASSIFICATIONS.map((option) => (
                <button
                  key={option.value}
                  type="button"
                  title={option.hint}
                  className={`btn${classification === option.value ? " btn--primary" : ""}`}
                  onClick={() => setClassification(option.value)}
                >
                  {option.label}
                </button>
              ))}
            </div>
          </div>
        )}
      </div>
    </Modal>
  );
}

function TemplateOption({
  template,
  selected,
  onSelect,
}: {
  template: TemplateSummary;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onSelect}
      aria-pressed={selected}
      className="card"
      style={{
        padding: "10px 12px",
        textAlign: "left",
        cursor: "pointer",
        borderColor: selected ? "var(--accent)" : undefined,
        background: selected ? "var(--accent-soft)" : undefined,
      }}
    >
      <div className="strong small">{template.name}</div>
      <div className="tiny muted" style={{ marginTop: 2 }}>
        {template.description}
      </div>
      <div className="tiny faint" style={{ marginTop: 5 }}>
        {template.section_count} sections · {template.question_count} questions
        {template.source === "user" ? " · custom template" : ""}
      </div>
    </button>
  );
}
