/** Postmortem list and creation. */

import { useEffect, useState } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";

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
import { useAction, useAsync } from "@/hooks/useAsync";
import { api } from "@/services/api";
import { formatRelative, humanize } from "@/services/format";

export function PostmortemsPage() {
  const navigate = useNavigate();
  const [params, setParams] = useSearchParams();
  const [creating, setCreating] = useState(params.get("new") === "1");
  const postmortems = useAsync(() => api.listPostmortems(100), []);
  const questions = useAsync(() => api.postmortemQuestions(), []);

  useEffect(() => {
    if (params.get("new") === "1") {
      setCreating(true);
      params.delete("new");
      setParams(params, { replace: true });
    }
  }, [params, setParams]);

  return (
    <div className="page stack">
      <PageHeader
        title="Postmortems"
        subtitle={`A blameless, guided review: ${questions.data?.length ?? 15} questions, an owner and a date for every corrective action.`}
        actions={
          <Button variant="primary" onClick={() => setCreating(true)}>
            New postmortem
          </Button>
        }
      />

      {postmortems.error && <Notice tone="danger">{postmortems.error.message}</Notice>}

      <Card flush>
        {postmortems.loading && !postmortems.data ? (
          <LoadingBlock />
        ) : !postmortems.data || postmortems.data.length === 0 ? (
          <EmptyState
            title="No postmortems yet"
            hint="Start one after an incident. The questions are fixed, so nothing important gets skipped."
            action={
              <Button variant="primary" onClick={() => setCreating(true)}>
                Start a postmortem
              </Button>
            }
          />
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>Reference</th>
                <th>Title</th>
                <th>Status</th>
                <th>Answered</th>
                <th>Corrective actions</th>
                <th className="right">Updated</th>
              </tr>
            </thead>
            <tbody>
              {postmortems.data.map((postmortem) => {
                const answered = Object.values(postmortem.answers).filter((answer) =>
                  answer.value.trim(),
                ).length;
                const total = questions.data?.length ?? 15;
                return (
                  <tr
                    key={postmortem.id}
                    data-clickable="true"
                    onClick={() => navigate(`/postmortems/${postmortem.id}`)}
                  >
                    <td className="table__ref">{postmortem.postmortem_ref}</td>
                    <td className="table__main table__truncate">{postmortem.title}</td>
                    <td className="muted nowrap">{humanize(postmortem.status)}</td>
                    <td className="muted nowrap">
                      {answered} / {total}
                    </td>
                    <td className="muted nowrap">{postmortem.corrective_actions.length}</td>
                    <td className="table__num muted nowrap tiny">
                      {formatRelative(postmortem.updated_at)}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </Card>

      {creating && (
        <CreatePostmortemDialog
          onClose={() => setCreating(false)}
          onCreated={(id) => navigate(`/postmortems/${id}`)}
        />
      )}
    </div>
  );
}

function CreatePostmortemDialog({
  onClose,
  onCreated,
}: {
  onClose: () => void;
  onCreated: (id: string) => void;
}) {
  const toast = useToast();
  const incidents = useAsync(() => api.listIncidents({ limit: 200 }), []);
  const [title, setTitle] = useState("");
  const [incidentId, setIncidentId] = useState<string | null>(null);

  const [create, { busy, error }] = useAction(async () =>
    api.createPostmortem({ title, incident_id: incidentId }),
  );

  return (
    <Modal
      title="New postmortem"
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button
            variant="primary"
            busy={busy}
            onClick={async () => {
              const created = await create();
              if (created) {
                toast.success(`${created.postmortem_ref} created`);
                onCreated(created.id);
              }
            }}
          >
            Start postmortem
          </Button>
        </>
      }
    >
      <div className="stack">
        {error && <Notice tone="danger">{error.message}</Notice>}
        <TextField
          label="Title"
          value={title}
          onValueChange={setTitle}
          placeholder="Jump host compromise, 4 September"
          required
          autoFocus
        />
        <div className="field">
          <label className="field__label" htmlFor="postmortem-incident">
            Related incident
          </label>
          <select
            id="postmortem-incident"
            className="select"
            value={incidentId ?? ""}
            onChange={(event) => setIncidentId(event.target.value || null)}
          >
            <option value="">Not linked</option>
            {incidents.data?.map((incident) => (
              <option key={incident.id} value={incident.id}>
                {incident.incident_ref} — {incident.title}
              </option>
            ))}
          </select>
        </div>
      </div>
    </Modal>
  );
}
