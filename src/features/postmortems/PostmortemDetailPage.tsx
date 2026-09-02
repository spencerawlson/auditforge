/**
 * The guided postmortem.
 *
 * A fixed question set rather than a blank document, a visible completion meter, and
 * corrective actions that will not be marked verified until someone says how they were
 * verified.
 */

import { useEffect, useMemo, useState } from "react";
import { useParams } from "react-router-dom";
import { openPath } from "@tauri-apps/plugin-opener";

import { ProvenanceBadge } from "@/components/badges";
import { CompletenessPanel } from "@/components/CompletenessPanel";
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
import { AiAssist } from "@/features/ai/AiAssist";
import { useAction, useAsync } from "@/hooks/useAsync";
import { api } from "@/services/api";
import { formatDate, humanize } from "@/services/format";
import type {
  CorrectiveActionStatus,
  PostmortemAnswerInput,
  PostmortemStatus,
  Provenance,
} from "@/types";

const STATUS_OPTIONS: Array<{ value: PostmortemStatus; label: string }> = [
  { value: "draft", label: "Draft" },
  { value: "review", label: "In review" },
  { value: "complete", label: "Complete" },
];

const ACTION_STATUS_OPTIONS: Array<{ value: CorrectiveActionStatus; label: string }> = [
  { value: "open", label: "Open" },
  { value: "in_progress", label: "In progress" },
  { value: "complete", label: "Complete" },
  { value: "verified", label: "Verified" },
  { value: "cancelled", label: "Cancelled" },
];

export function PostmortemDetailPage() {
  const { id = "" } = useParams();
  const toast = useToast();
  const postmortem = useAsync(() => api.getPostmortem(id), [id]);
  const questions = useAsync(() => api.postmortemQuestions(), []);
  const completeness = useAsync(() => api.postmortemCompleteness(id), [id]);

  const [answers, setAnswers] = useState<Record<string, { value: string; provenance: Provenance }>>(
    {},
  );
  const [addingAction, setAddingAction] = useState(false);

  useEffect(() => {
    if (!postmortem.data) return;
    const next: Record<string, { value: string; provenance: Provenance }> = {};
    for (const [key, answer] of Object.entries(postmortem.data.answers)) {
      next[key] = { value: answer.value, provenance: answer.provenance };
    }
    setAnswers(next);
  }, [postmortem.data]);

  const [save, saveState] = useAction(async () => {
    const payload: PostmortemAnswerInput[] = Object.entries(answers).map(([key, answer]) => ({
      question_key: key,
      value: answer.value,
      provenance: answer.provenance,
    }));
    const updated = await api.updatePostmortem(id, { answers: payload });
    postmortem.set(updated);
    completeness.reload();
    return updated;
  });

  const [setStatus, statusState] = useAction(async (status: PostmortemStatus) => {
    const updated = await api.updatePostmortem(id, { status });
    postmortem.set(updated);
    completeness.reload();
    return updated;
  });

  const [exportPdf, exportState] = useAction(async () => {
    const path = await api.renderPostmortemPdf(id);
    await openPath(path);
    return path;
  });

  const completion = useMemo(() => {
    const total = questions.data?.length ?? 0;
    const answered = (questions.data ?? []).filter((question) =>
      (answers[question.key]?.value ?? "").trim(),
    ).length;
    return { answered, total };
  }, [answers, questions.data]);

  if (postmortem.loading && !postmortem.data) return <LoadingBlock />;
  if (postmortem.error) return <Notice tone="danger">{postmortem.error.message}</Notice>;
  if (!postmortem.data) return null;
  const record = postmortem.data;
  const locked = record.status === "complete";

  return (
    <div className="page stack">
      <PageHeader
        title={record.title}
        subtitle={
          <span className="row row--wrap">
            <span className="mono">{record.postmortem_ref}</span>
            <span className="badge badge--outline">{humanize(record.status)}</span>
            <span className="muted">
              {completion.answered} of {completion.total} questions answered
            </span>
          </span>
        }
        actions={
          <div className="row">
            <Button
              busy={exportState.busy}
              onClick={() =>
                void exportPdf().then((path) => {
                  if (path) toast.success("Postmortem PDF generated");
                })
              }
            >
              Export PDF
            </Button>
            <Button
              variant="primary"
              busy={saveState.busy}
              disabled={locked}
              onClick={() => void save().then(() => toast.success("Postmortem saved"))}
            >
              Save answers
            </Button>
          </div>
        }
      />

      {locked && (
        <Notice tone="info" title="Marked complete">
          Set the status back to Draft or In review to make further changes.
        </Notice>
      )}
      {saveState.error && <Notice tone="danger">{saveState.error.message}</Notice>}
      {statusState.error && <Notice tone="danger">{statusState.error.message}</Notice>}
      {exportState.error && <Notice tone="danger">{exportState.error.message}</Notice>}

      {completeness.data && (
        <Card title="Completeness">
          <CompletenessPanel completeness={completeness.data} />
        </Card>
      )}

      <div className="row" style={{ maxWidth: 240 }}>
        <SelectField
          label="Status"
          value={record.status}
          options={STATUS_OPTIONS}
          disabled={statusState.busy}
          onValueChange={(status) => void setStatus(status)}
        />
      </div>

      <Card title="Guided questions">
        <div className="stack">
          {questions.loading ? (
            <LoadingBlock />
          ) : (
            questions.data?.map((question) => {
              const answer = answers[question.key] ?? { value: "", provenance: "user_provided" as Provenance };
              return (
                <div className="question" key={question.key}>
                  <div className="question__head">
                    <span className="question__label">{question.prompt}</span>
                    <ProvenanceBadge provenance={answer.provenance} />
                  </div>
                  <textarea
                    className="textarea"
                    value={answer.value}
                    disabled={locked}
                    aria-label={question.prompt}
                    onChange={(event) =>
                      setAnswers((current) => ({
                        ...current,
                        [question.key]: {
                          value: event.target.value,
                          provenance: "user_provided",
                        },
                      }))
                    }
                  />
                  <div className="question__toolbar">
                    <AiAssist
                      text={answer.value}
                      context={`Postmortem ${record.postmortem_ref}: ${record.title}. Question: ${question.prompt}`}
                      targetType="postmortem"
                      targetId={record.id}
                      disabled={locked}
                      onApply={(value, provenance) =>
                        setAnswers((current) => ({
                          ...current,
                          [question.key]: { value, provenance },
                        }))
                      }
                    />
                  </div>
                </div>
              );
            })
          )}
        </div>
      </Card>

      <Card
        title="Corrective actions"
        actions={
          <Button size="sm" disabled={locked} onClick={() => setAddingAction(true)}>
            Add action
          </Button>
        }
        flush
      >
        {record.corrective_actions.length === 0 ? (
          <EmptyState
            title="No corrective actions"
            hint="Every action needs an owner, a date and a way to verify it."
          />
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>Reference</th>
                <th>Action</th>
                <th>Owner</th>
                <th>Due</th>
                <th>Status</th>
              </tr>
            </thead>
            <tbody>
              {record.corrective_actions.map((action) => (
                <tr key={action.id}>
                  <td className="table__ref">{action.action_ref}</td>
                  <td className="table__main table__truncate">{action.description}</td>
                  <td className="muted">{action.owner || "Unassigned"}</td>
                  <td className="muted nowrap">{formatDate(action.due_date)}</td>
                  <td>
                    <ActionStatusControl
                      action={action}
                      disabled={locked}
                      onUpdated={() => postmortem.reload()}
                    />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Card>

      {addingAction && (
        <AddActionDialog
          postmortemId={record.id}
          onClose={() => setAddingAction(false)}
          onAdded={() => {
            setAddingAction(false);
            postmortem.reload();
            toast.success("Corrective action added");
          }}
        />
      )}
    </div>
  );
}

function ActionStatusControl({
  action,
  disabled,
  onUpdated,
}: {
  action: import("@/types").CorrectiveAction;
  disabled: boolean;
  onUpdated: () => void;
}) {
  const toast = useToast();
  const [verifying, setVerifying] = useState(false);
  const [method, setMethod] = useState(action.verification_method);

  const [update, { busy, error }] = useAction(
    async (status: CorrectiveActionStatus, verification?: string) => {
      await api.updateCorrectiveAction(action.id, {
        status,
        ...(verification !== undefined ? { verification_method: verification } : {}),
      });
      onUpdated();
      return true;
    },
  );

  return (
    <>
      <select
        className="select"
        style={{ maxWidth: 150, height: 28, padding: "0 8px" }}
        value={action.status}
        disabled={disabled || busy}
        aria-label={`Status of ${action.action_ref}`}
        onChange={(event) => {
          const next = event.target.value as CorrectiveActionStatus;
          // Verification needs a method recorded; the backend refuses without one.
          if (next === "verified" && !action.verification_method.trim()) {
            setVerifying(true);
            return;
          }
          void update(next);
        }}
      >
        {ACTION_STATUS_OPTIONS.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>

      {error && <div className="tiny" style={{ color: "var(--danger)" }}>{error.message}</div>}

      {verifying && (
        <Modal
          title="How was this verified?"
          onClose={() => setVerifying(false)}
          footer={
            <>
              <Button onClick={() => setVerifying(false)}>Cancel</Button>
              <Button
                variant="primary"
                busy={busy}
                disabled={!method.trim()}
                onClick={async () => {
                  const ok = await update("verified", method);
                  if (ok) {
                    setVerifying(false);
                    toast.success("Marked verified");
                  }
                }}
              >
                Mark verified
              </Button>
            </>
          }
        >
          <div className="stack">
            <p className="small muted">
              An action cannot be marked verified without recording how. This is what you will
              point at when someone asks whether the fix actually worked.
            </p>
            <TextAreaField
              label="Verification method"
              value={method}
              onValueChange={setMethod}
              placeholder="Re-ran the authenticated scan on 2026-10-02; TLS 1.0 no longer offered."
            />
          </div>
        </Modal>
      )}
    </>
  );
}

function AddActionDialog({
  postmortemId,
  onClose,
  onAdded,
}: {
  postmortemId: string;
  onClose: () => void;
  onAdded: () => void;
}) {
  const [description, setDescription] = useState("");
  const [owner, setOwner] = useState("");
  const [dueDate, setDueDate] = useState("");
  const [verification, setVerification] = useState("");

  const [add, { busy, error }] = useAction(async () =>
    api.addCorrectiveAction({
      description,
      owner,
      due_date: dueDate || null,
      verification_method: verification,
      postmortem_id: postmortemId,
    }),
  );

  return (
    <Modal
      title="Add a corrective action"
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button
            variant="primary"
            busy={busy}
            disabled={!description.trim()}
            onClick={async () => {
              const created = await add();
              if (created) onAdded();
            }}
          >
            Add action
          </Button>
        </>
      }
    >
      <div className="stack">
        {error && <Notice tone="danger">{error.message}</Notice>}
        <TextAreaField
          label="Action"
          value={description}
          onValueChange={setDescription}
          placeholder="Rotate all jump host credentials and enforce MFA"
          required
        />
        <TextField label="Owner" value={owner} onValueChange={setOwner} placeholder="Platform team" />
        <TextField label="Due date" type="date" value={dueDate} onValueChange={setDueDate} />
        <TextAreaField
          label="How will it be verified?"
          value={verification}
          onValueChange={setVerification}
          help="Can be filled in later, but it is required before the action can be marked verified."
        />
      </div>
    </Modal>
  );
}
