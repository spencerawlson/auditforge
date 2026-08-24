/** Template browser: what each report type asks, before you commit to one. */

import { useState } from "react";

import { ClassificationBadge } from "@/components/badges";
import {
  Card,
  EmptyState,
  LoadingBlock,
  Notice,
  PageHeader,
} from "@/components/ui";
import { useAsync } from "@/hooks/useAsync";
import { api } from "@/services/api";
import { humanize } from "@/services/format";

export function TemplatesPage() {
  const templates = useAsync(() => api.listTemplates(), []);
  const [selected, setSelected] = useState<string | null>(null);
  const detail = useAsync(
    () => (selected ? api.getTemplate(selected) : Promise.resolve(null)),
    [selected],
  );

  return (
    <div className="page stack">
      <PageHeader
        title="Report templates"
        subtitle="A template defines a report's sections, its questions and which of them are required. Templates are data, not code."
      />

      {templates.error && <Notice tone="danger">{templates.error.message}</Notice>}

      <div className="grid grid--2">
        <Card title="Available types" flush>
          {templates.loading ? (
            <LoadingBlock />
          ) : (
            <table className="table">
              <tbody>
                {templates.data?.map((template) => (
                  <tr
                    key={template.key}
                    data-clickable="true"
                    onClick={() => setSelected(template.key)}
                    style={
                      selected === template.key ? { background: "var(--accent-soft)" } : undefined
                    }
                  >
                    <td>
                      <div className="table__main">{template.name}</div>
                      <div className="tiny muted">{template.description}</div>
                    </td>
                    <td className="table__num tiny muted nowrap">
                      {template.section_count} sections
                      <br />
                      {template.question_count} questions
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </Card>

        <Card title={detail.data ? detail.data.name : "Select a type"}>
          {!selected ? (
            <EmptyState
              title="Nothing selected"
              hint="Choose a report type to see the sections and questions it will ask you."
            />
          ) : detail.loading ? (
            <LoadingBlock />
          ) : detail.data ? (
            <div className="stack">
              <div className="row row--wrap">
                <span className="badge badge--outline">{humanize(detail.data.report_type)}</span>
                <ClassificationBadge value={detail.data.default_classification} />
                <span className="badge badge--outline">
                  {detail.data.source === "user" ? "Custom template" : "Built in"}
                </span>
                <span className="badge badge--outline">v{detail.data.version}</span>
              </div>

              {detail.data.sections.map((section, index) => (
                <div key={section.key} style={{ borderTop: "1px solid var(--line)", paddingTop: 10 }}>
                  <div className="row">
                    <span className="strong small">
                      {index + 1}. {section.title}
                    </span>
                    {section.required && <span className="badge badge--outline">Required</span>}
                    {section.kind !== "guided" && (
                      <span className="badge badge--accent">
                        Built from your {section.kind === "findings" ? "findings" : "evidence"}
                      </span>
                    )}
                  </div>
                  {section.guidance && (
                    <p className="tiny muted" style={{ marginTop: 3 }}>
                      {section.guidance}
                    </p>
                  )}
                  {section.questions.length > 0 && (
                    <ul className="tiny muted" style={{ margin: "6px 0 0", paddingLeft: 18 }}>
                      {section.questions.map((question) => (
                        <li key={question.key}>
                          {question.label}
                          {question.required && (
                            <span style={{ color: "var(--danger)" }}> *</span>
                          )}
                        </li>
                      ))}
                    </ul>
                  )}
                </div>
              ))}
            </div>
          ) : null}
        </Card>
      </div>

      <Notice tone="info" title="Custom templates">
        Drop a template JSON file into the <span className="mono">templates</span> folder inside
        your AuditForge data directory and restart the application. Its structure matches the
        built-in files; see <span className="mono">docs/templates.md</span>. A custom template with
        the same key as a built-in one is never overwritten by an update.
      </Notice>
    </div>
  );
}
