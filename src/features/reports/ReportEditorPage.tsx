/**
 * The guided report editor.
 *
 * Left: the section outline, with a dot showing what still needs attention. Middle: the
 * questions for the current section. Right: readiness, AI review, actions and history.
 *
 * The author answers questions. Nothing here lets them lay out a document, because that
 * is the renderer's job.
 */

import { useCallback, useEffect, useMemo, useState } from "react";
import { useParams } from "react-router-dom";

import { ClassificationBadge, ProvenanceBadge, ReportStatusBadge } from "@/components/badges";
import { TableAnswer } from "@/components/TableAnswer";
import { useToast } from "@/components/Toast";
import { Button, Card, LoadingBlock, Notice, PageHeader } from "@/components/ui";
import { AiAssist } from "@/features/ai/AiAssist";
import { useAction, useAsync } from "@/hooks/useAsync";
import { api } from "@/services/api";
import { formatDateTime } from "@/services/format";
import type {
  AnswerInput,
  Provenance,
  ReportWorkspace,
  SectionContent,
  TemplateSection,
} from "@/types";

import { EvidencePicker, FindingPicker } from "./ReportPickers";
import { AiReviewPanel, ExportPanel, FinalizePanel, VersionHistory } from "./ReportSidePanels";

interface DraftAnswer {
  value: string;
  provenance: Provenance;
  aiRequestId: string | null;
}

export function ReportEditorPage() {
  const { id = "" } = useParams();
  const toast = useToast();
  const workspace = useAsync(() => api.openReport(id), [id]);

  const [activeSection, setActiveSection] = useState<string | null>(null);
  const [drafts, setDrafts] = useState<Record<string, DraftAnswer>>({});
  const [titleDraft, setTitleDraft] = useState<string | null>(null);

  const data = workspace.data;
  const editable = data ? !data.version.is_immutable && data.version.status !== "final" : false;

  useEffect(() => {
    if (!data || activeSection) return;
    const first = data.version.content.sections.find((section) => section.included);
    setActiveSection(first?.key ?? null);
  }, [data, activeSection]);

  // Any successful write returns the new version, so the screen re-reads the workspace
  // rather than trying to patch its own copy.
  const refresh = useCallback(async () => {
    workspace.reload();
    setDrafts({});
    setTitleDraft(null);
  }, [workspace]);

  const [save, saveState] = useAction(async () => {
    const answers: AnswerInput[] = Object.entries(drafts).map(([key, draft]) => {
      const [sectionKey, questionKey] = key.split("::");
      return {
        section_key: sectionKey ?? "",
        question_key: questionKey ?? "",
        value: draft.value,
        provenance: draft.provenance,
        ai_request_id: draft.aiRequestId,
      };
    });
    await api.saveReportDraft(id, {
      ...(titleDraft !== null ? { title: titleDraft } : {}),
      ...(answers.length > 0 ? { answers } : {}),
    });
    return true;
  });

  const dirty = Object.keys(drafts).length > 0 || titleDraft !== null;

  // Ctrl/Cmd+S saves, which is what everyone tries first in a document editor.
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
        event.preventDefault();
        if (dirty && editable) {
          void save().then((ok) => {
            if (ok) {
              toast.success("Draft saved");
              void refresh();
            }
          });
        }
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [dirty, editable, save, refresh, toast]);

  if (workspace.loading && !data) return <LoadingBlock label="Opening report" />;
  if (workspace.error) return <Notice tone="danger">{workspace.error.message}</Notice>;
  if (!data || !activeSection) return null;

  const section = data.version.content.sections.find((item) => item.key === activeSection);
  const templateSection = data.template?.sections.find((item) => item.key === activeSection);

  function answerKey(sectionKey: string, questionKey: string) {
    return `${sectionKey}::${questionKey}`;
  }

  function currentAnswer(sectionKey: string, questionKey: string): DraftAnswer {
    const key = answerKey(sectionKey, questionKey);
    const draft = drafts[key];
    if (draft) return draft;
    const stored = data!.version.content.sections
      .find((item) => item.key === sectionKey)
      ?.answers.find((item) => item.question_key === questionKey);
    return {
      value: stored?.value ?? "",
      provenance: stored?.provenance ?? "user_provided",
      aiRequestId: stored?.ai_request_id ?? null,
    };
  }

  function setAnswer(sectionKey: string, questionKey: string, next: Partial<DraftAnswer>) {
    const key = answerKey(sectionKey, questionKey);
    setDrafts((current) => {
      const base = current[key] ?? currentAnswer(sectionKey, questionKey);
      return { ...current, [key]: { ...base, ...next } };
    });
  }

  return (
    <div className="page page--wide stack">
      <PageHeader
        title={
          editable ? (
            <input
              className="input"
              style={{ fontSize: "var(--text-xl)", fontWeight: 650, border: "1px solid transparent", padding: "2px 6px" }}
              value={titleDraft ?? data.version.content.title}
              onChange={(event) => setTitleDraft(event.target.value)}
              aria-label="Report title"
            />
          ) : (
            data.version.content.title
          )
        }
        subtitle={
          <span className="row row--wrap">
            <span className="mono">{data.report.report_ref}</span>
            <ReportStatusBadge status={data.version.status} />
            <span className="badge badge--outline">{data.version.version_label}</span>
            <ClassificationBadge value={data.version.content.classification} />
            {data.report.engagement_id && data.version.content.engagement.name && (
              <span className="muted">· {data.version.content.engagement.name}</span>
            )}
          </span>
        }
        actions={
          <>
            <Button
              busy={saveState.busy}
              disabled={!dirty || !editable}
              onClick={async () => {
                const ok = await save();
                if (ok) {
                  toast.success("Draft saved");
                  void refresh();
                }
              }}
            >
              {dirty ? "Save draft" : "Saved"}
            </Button>
          </>
        }
      />

      {saveState.error && <Notice tone="danger">{saveState.error.message}</Notice>}

      {!editable && (
        <Notice tone="info" title={`${data.version.version_label} is final and cannot be edited`}>
          Finalized versions are immutable, in the application and in the database. To make a
          correction, create a new revision: the issued document stays exactly as it was.
        </Notice>
      )}

      <div
        className="card"
        style={{ display: "grid", gridTemplateColumns: "230px minmax(0, 1fr) 330px", minHeight: 560 }}
      >
        <nav className="editor__outline" aria-label="Report sections">
          <Outline
            workspace={data}
            active={activeSection}
            onSelect={setActiveSection}
            dirtyKeys={Object.keys(drafts)}
          />
        </nav>

        <div className="editor__main">
          {section && templateSection ? (
            <SectionEditor
              section={section}
              template={templateSection}
              editable={editable}
              reportId={id}
              reportRef={data.report.report_ref}
              currentAnswer={currentAnswer}
              setAnswer={setAnswer}
              workspace={data}
              onChanged={refresh}
            />
          ) : section ? (
            <UntemplatedSection section={section} />
          ) : null}
        </div>

        <aside className="editor__aside">
          <div className="stack">
            <FinalizePanel workspace={data} onChanged={refresh} dirty={dirty} />
            <AiReviewPanel reportId={id} onJumpToSection={setActiveSection} />
            <ExportPanel workspace={data} />
            <VersionHistory workspace={data} />
          </div>
        </aside>
      </div>

      <Card title="Document details">
        <div className="grid grid--3">
          <div>
            <div className="compare__label">Author</div>
            <div className="small strong">{data.version.content.author.full_name}</div>
            <div className="tiny muted">{data.version.content.author.job_title}</div>
            <div className="tiny faint">{data.version.content.author.organization}</div>
          </div>
          <div>
            <div className="compare__label">Version</div>
            <div className="small">{data.version.version_label}</div>
            <div className="tiny muted">Saved {formatDateTime(data.version.created_at)}</div>
          </div>
          <div>
            <div className="compare__label">Content hash</div>
            <div className="mono tiny">{data.version.content_hash.slice(0, 32)}…</div>
          </div>
        </div>
      </Card>
    </div>
  );
}

// ---------------------------------------------------------------- outline

function Outline({
  workspace,
  active,
  onSelect,
  dirtyKeys,
}: {
  workspace: ReportWorkspace;
  active: string;
  onSelect: (key: string) => void;
  dirtyKeys: string[];
}) {
  const blockers = useMemo(() => {
    const map = new Map<string, "blocker" | "warning">();
    for (const issue of workspace.readiness.issues) {
      if (!issue.section_key) continue;
      const existing = map.get(issue.section_key);
      if (issue.severity === "blocker" || existing !== "blocker") {
        map.set(issue.section_key, issue.severity);
      }
    }
    return map;
  }, [workspace.readiness.issues]);

  let number = 0;
  return (
    <div className="stack stack--sm">
      {workspace.version.content.sections.map((section) => {
        if (section.included) number += 1;
        const state = blockers.get(section.key);
        const hasDirty = dirtyKeys.some((key) => key.startsWith(`${section.key}::`));
        const done = section.included && !state && section.answers.some((answer) => answer.value.trim());
        return (
          <button
            key={section.key}
            type="button"
            className={`outline-item${section.key === active ? " outline-item--active" : ""}${
              section.included ? "" : " outline-item--excluded"
            }`}
            onClick={() => onSelect(section.key)}
          >
            <span className="outline-item__num">{section.included ? number : "—"}</span>
            <span className="table__truncate">{section.title}</span>
            {hasDirty && <span className="tiny faint">•</span>}
            {state && <span className={`outline-item__dot outline-item__dot--${state}`} />}
            {!state && done && <span className="outline-item__dot outline-item__dot--done" />}
          </button>
        );
      })}
    </div>
  );
}

// ---------------------------------------------------------------- section editor

function SectionEditor({
  section,
  template,
  editable,
  reportId,
  reportRef,
  currentAnswer,
  setAnswer,
  workspace,
  onChanged,
}: {
  section: SectionContent;
  template: TemplateSection;
  editable: boolean;
  reportId: string;
  reportRef: string;
  currentAnswer: (sectionKey: string, questionKey: string) => DraftAnswer;
  setAnswer: (sectionKey: string, questionKey: string, next: Partial<DraftAnswer>) => void;
  workspace: ReportWorkspace;
  onChanged: () => Promise<void>;
}) {
  const toast = useToast();
  const [toggleInclusion, toggleState] = useAction(async (include: boolean) => {
    const included = workspace.version.content.sections
      .filter((item) => (item.key === section.key ? include : item.included))
      .map((item) => item.key);
    await api.saveReportDraft(reportId, { included_sections: included });
    await onChanged();
    return true;
  });

  return (
    <div className="stack">
      <div className="row row--between">
        <div>
          <h2 style={{ fontSize: "var(--text-lg)", fontWeight: 650 }}>{section.title}</h2>
          {template.guidance && (
            <p className="small muted" style={{ maxWidth: "78ch", marginTop: 4 }}>
              {template.guidance}
            </p>
          )}
        </div>
        {editable && !template.required && (
          <Button
            size="sm"
            variant="ghost"
            busy={toggleState.busy}
            onClick={async () => {
              const ok = await toggleInclusion(!section.included);
              if (ok) {
                toast.push(
                  section.included
                    ? "Section excluded. Its text is kept in case you switch it back on."
                    : "Section included.",
                );
              }
            }}
          >
            {section.included ? "Exclude from report" : "Include in report"}
          </Button>
        )}
      </div>

      {toggleState.error && <Notice tone="danger">{toggleState.error.message}</Notice>}

      {!section.included && (
        <Notice tone="neutral">
          This section is excluded and will not appear in the document. Anything written here is
          kept.
        </Notice>
      )}

      {template.kind === "findings" && (
        <FindingPicker workspace={workspace} editable={editable} onChanged={onChanged} />
      )}

      {template.kind === "evidence" && (
        <EvidencePicker workspace={workspace} editable={editable} onChanged={onChanged} />
      )}

      {template.kind === "guided" &&
        template.questions.map((question) => {
          const answer = currentAnswer(section.key, question.key);
          return (
            <div className="question" key={question.key}>
              <div className="question__head">
                <span className="question__label">{question.label}</span>
                {question.required && <span className="badge badge--outline">Required</span>}
                <ProvenanceBadge provenance={answer.provenance} />
              </div>
              {question.help && <p className="question__help">{question.help}</p>}

              {question.input === "table" ? (
                <TableAnswer
                  columns={question.columns ?? []}
                  value={answer.value}
                    disabled={!editable}
                  rowLabel={question.row_label}
                  label={question.label}
                  onChange={(value) =>
                    setAnswer(section.key, question.key, {
                      value,
                      provenance: "user_provided",
                      aiRequestId: null,
                    })
                  }
                />
              ) : question.input === "date" ? (
                <input
                  className="input"
                  type="date"
                  style={{ maxWidth: 220 }}
                  value={answer.value}
                  disabled={!editable}
                  onChange={(event) =>
                    setAnswer(section.key, question.key, {
                      value: event.target.value,
                      provenance: "user_provided",
                      aiRequestId: null,
                    })
                  }
                  aria-label={question.label}
                />
              ) : question.input === "text" ? (
                <input
                  className="input"
                  value={answer.value}
                  placeholder={question.placeholder}
                  disabled={!editable}
                  onChange={(event) =>
                    setAnswer(section.key, question.key, {
                      value: event.target.value,
                      provenance: "user_provided",
                      aiRequestId: null,
                    })
                  }
                  aria-label={question.label}
                />
              ) : (
                <textarea
                  className="textarea"
                  value={answer.value}
                  placeholder={question.placeholder}
                  disabled={!editable}
                  onChange={(event) =>
                    setAnswer(section.key, question.key, {
                      value: event.target.value,
                      provenance: "user_provided",
                      aiRequestId: null,
                    })
                  }
                  aria-label={question.label}
                />
              )}

              <div className="question__toolbar">
                {question.input !== "table" && (
                  <AiAssist
                    text={answer.value}
                    context={`${reportRef} — ${section.title} — ${question.label}`}
                    targetType="report"
                    targetId={reportId}
                  disabled={!editable}
                    onApply={(value, provenance, requestId) =>
                      setAnswer(section.key, question.key, {
                        value,
                        provenance,
                        aiRequestId: requestId,
                      })
                    }
                  />
                )}
                {question.input !== "table" && editable && answer.value.trim().length > 0 && (
                  <Button
                    size="sm"
                    variant="ghost"
                    title="Mark this as a known gap. It prints as “Information required.”"
                    onClick={() =>
                      setAnswer(section.key, question.key, { provenance: "missing_information" })
                    }
                  >
                    Mark as a gap
                  </Button>
                )}
                {answer.provenance === "ai_inference" && editable && (
                  <Button
                    size="sm"
                    variant="ghost"
                    title="Confirm this wording as your own. Required before the report can be finalized."
                    onClick={() =>
                      setAnswer(section.key, question.key, { provenance: "ai_wording" })
                    }
                  >
                    Confirm as mine
                  </Button>
                )}
                {question.input !== "table" && (
                  <span className="tiny faint">{answer.value.length} characters</span>
                )}
              </div>
            </div>
          );
        })}
    </div>
  );
}

/** A section stored on the report but no longer present in its template. */
function UntemplatedSection({ section }: { section: SectionContent }) {
  return (
    <div className="stack">
      <h2 style={{ fontSize: "var(--text-lg)", fontWeight: 650 }}>{section.title}</h2>
      <Notice tone="warning" title="This section is no longer part of the template">
        Its content is preserved and will still be rendered. It is shown read-only because the
        template no longer describes its questions.
      </Notice>
      {section.answers.map((answer) => (
        <div key={answer.question_key}>
          <div className="compare__label">{answer.question_key}</div>
          <p className="prewrap small">{answer.value}</p>
        </div>
      ))}
    </div>
  );
}
