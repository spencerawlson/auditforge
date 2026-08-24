/** The landing screen: what is in flight, what needs attention, and what to start next. */

import { useNavigate } from "react-router-dom";

import { ReportStatusBadge, SeverityBadge, SeverityConfirmationBadge } from "@/components/badges";
import { Button, Card, EmptyState, LoadingBlock, Notice, PageHeader, Stat } from "@/components/ui";
import { useSession } from "@/features/auth/SessionProvider";
import { useAsync } from "@/hooks/useAsync";
import { api } from "@/services/api";
import { formatRelative, truncate } from "@/services/format";
import type { Finding, ReportSummary } from "@/types";

export function DashboardPage() {
  const navigate = useNavigate();
  const { user } = useSession();
  const dashboard = useAsync(() => api.dashboard(), []);

  if (dashboard.loading && !dashboard.data) return <LoadingBlock label="Loading your workspace" />;
  if (dashboard.error) {
    return <Notice tone="danger">{dashboard.error.message}</Notice>;
  }
  if (!dashboard.data) return null;

  const { counts, recent_reports, drafts, awaiting_review, ready_to_export, active_engagements, recent_findings, recent_postmortems } =
    dashboard.data;

  const empty = counts.reports === 0 && counts.engagements === 0 && counts.findings === 0;

  return (
    <div className="page stack stack--lg">
      <PageHeader
        title={`Welcome back, ${user?.full_name.split(" ")[0] ?? ""}`}
        subtitle="Everything below is stored locally on this machine."
        actions={
          <>
            <Button onClick={() => navigate("/engagements?new=1")}>New engagement</Button>
            <Button onClick={() => navigate("/findings?new=1")}>New finding</Button>
            <Button onClick={() => navigate("/postmortems?new=1")}>New postmortem</Button>
            <Button variant="primary" onClick={() => navigate("/reports?new=1")}>
              New report
            </Button>
          </>
        }
      />

      {empty && (
        <Notice tone="info" title="Nothing here yet">
          Start with an engagement to group the work, then create a report inside it. A report
          asks you guided questions and turns your answers into a formatted document; you never
          have to lay anything out yourself.
        </Notice>
      )}

      <div className="grid grid--4">
        <Stat value={counts.engagements} label="Engagements" />
        <Stat value={counts.reports} label="Reports" />
        <Stat value={counts.findings} label="Findings" />
        <Stat value={counts.evidence} label="Evidence items" />
      </div>

      {counts.unconfirmed_severities > 0 && (
        <Notice tone="warning" title="Severity decisions outstanding">
          {counts.unconfirmed_severities} finding
          {counts.unconfirmed_severities === 1 ? "" : "s"} still need a severity confirmed by a
          person. A report cannot be finalized while it includes one.{" "}
          <button className="btn btn--ghost btn--sm" onClick={() => navigate("/findings")}>
            Review findings
          </button>
        </Notice>
      )}

      <div className="grid grid--2">
        <Card
          title="Drafts"
          actions={
            <Button size="sm" variant="ghost" onClick={() => navigate("/reports")}>
              All reports
            </Button>
          }
          flush
        >
          <ReportList reports={drafts} empty="No drafts in progress." onOpen={(id) => navigate(`/reports/${id}`)} />
        </Card>

        <Card title="Awaiting review" flush>
          <ReportList
            reports={awaiting_review}
            empty="Nothing is waiting for review."
            onOpen={(id) => navigate(`/reports/${id}`)}
          />
        </Card>

        <Card title="Ready to export" flush>
          <ReportList
            reports={ready_to_export}
            empty="No finalized reports yet."
            onOpen={(id) => navigate(`/reports/${id}`)}
          />
        </Card>

        <Card title="Recent activity" flush>
          <ReportList
            reports={recent_reports}
            empty="No reports yet."
            onOpen={(id) => navigate(`/reports/${id}`)}
            showStatus
          />
        </Card>
      </div>

      <div className="grid grid--2">
        <Card
          title="Active engagements"
          actions={
            <Button size="sm" variant="ghost" onClick={() => navigate("/engagements")}>
              All engagements
            </Button>
          }
          flush
        >
          {active_engagements.length === 0 ? (
            <EmptyState title="No active engagements" hint="Engagements start as drafts; set one to Active when work begins." />
          ) : (
            <table className="table">
              <tbody>
                {active_engagements.map((engagement) => (
                  <tr
                    key={engagement.id}
                    data-clickable="true"
                    onClick={() => navigate(`/engagements/${engagement.id}`)}
                  >
                    <td className="table__ref">{engagement.engagement_ref}</td>
                    <td className="table__main table__truncate">{engagement.name}</td>
                    <td className="muted nowrap">{engagement.client_name}</td>
                    <td className="table__num muted nowrap">
                      {engagement.report_count} rpt · {engagement.finding_count} fnd
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </Card>

        <Card
          title="Recent findings"
          actions={
            <Button size="sm" variant="ghost" onClick={() => navigate("/findings")}>
              All findings
            </Button>
          }
          flush
        >
          <FindingList findings={recent_findings} onOpen={(id) => navigate(`/findings/${id}`)} />
        </Card>
      </div>

      {recent_postmortems.length > 0 && (
        <Card title="Recent postmortems" flush>
          <table className="table">
            <tbody>
              {recent_postmortems.map((postmortem) => (
                <tr
                  key={postmortem.id}
                  data-clickable="true"
                  onClick={() => navigate(`/postmortems/${postmortem.id}`)}
                >
                  <td className="table__ref">{postmortem.postmortem_ref}</td>
                  <td className="table__main table__truncate">{postmortem.title}</td>
                  <td className="muted nowrap">{postmortem.status}</td>
                  <td className="muted nowrap right">{formatRelative(postmortem.updated_at)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      )}
    </div>
  );
}

function ReportList({
  reports,
  empty,
  onOpen,
  showStatus = false,
}: {
  reports: ReportSummary[];
  empty: string;
  onOpen: (id: string) => void;
  showStatus?: boolean;
}) {
  if (reports.length === 0) return <EmptyState title={empty} />;
  return (
    <table className="table">
      <tbody>
        {reports.map((report) => (
          <tr key={report.id} data-clickable="true" onClick={() => onOpen(report.id)}>
            <td className="table__ref">{report.report_ref}</td>
            <td className="table__main table__truncate">{report.title}</td>
            {showStatus && (
              <td className="nowrap">
                <ReportStatusBadge status={report.status} />
              </td>
            )}
            <td className="muted nowrap">{report.current_version_label ?? "—"}</td>
            <td className="muted nowrap right">{formatRelative(report.updated_at)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function FindingList({ findings, onOpen }: { findings: Finding[]; onOpen: (id: string) => void }) {
  if (findings.length === 0) {
    return <EmptyState title="No findings yet" hint="Findings are reusable: record one once and include it in any report." />;
  }
  return (
    <table className="table">
      <tbody>
        {findings.map((finding) => (
          <tr key={finding.id} data-clickable="true" onClick={() => onOpen(finding.id)}>
            <td className="nowrap">
              <SeverityBadge severity={finding.severity} />
            </td>
            <td className="table__main table__truncate">{truncate(finding.title, 70)}</td>
            <td className="nowrap">
              {!finding.severity_confirmed && <SeverityConfirmationBadge confirmed={false} />}
            </td>
            <td className="muted nowrap right">{formatRelative(finding.updated_at)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
