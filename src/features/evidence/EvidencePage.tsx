/**
 * The evidence vault.
 *
 * Import copies the file in, records its SHA-256, and never opens it. "Show in folder"
 * hands the path to the desktop rather than rendering anything, because evidence is by
 * definition material of unknown provenance.
 */

import { useState } from "react";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import { Modal } from "@/components/Modal";
import { useToast } from "@/components/Toast";
import {
  Button,
  Card,
  Checkbox,
  EmptyState,
  KeyValues,
  LoadingBlock,
  Notice,
  PageHeader,
  TextAreaField,
  TextField,
} from "@/components/ui";
import { useAction, useAsync, useDebouncedValue } from "@/hooks/useAsync";
import { api } from "@/services/api";
import { formatBytes, formatDateTime, humanize } from "@/services/format";
import type { Evidence, EvidenceKind } from "@/types";

const KIND_FILTERS: Array<{ value: EvidenceKind | "all"; label: string }> = [
  { value: "all", label: "All types" },
  { value: "image", label: "Images" },
  { value: "log", label: "Logs" },
  { value: "text", label: "Text" },
  { value: "csv", label: "CSV" },
  { value: "json", label: "JSON" },
  { value: "capture", label: "Packet captures" },
  { value: "archive", label: "Archives" },
  { value: "other", label: "Other" },
];

export function EvidencePage() {
  const [search, setSearch] = useState("");
  const [kind, setKind] = useState<EvidenceKind | "all">("all");
  const [importing, setImporting] = useState(false);
  const [inspecting, setInspecting] = useState<Evidence | null>(null);
  const query = useDebouncedValue(search);

  const evidence = useAsync(
    () =>
      api.listEvidence({
        ...(query.trim() ? { search: query.trim() } : {}),
        ...(kind === "all" ? {} : { kind }),
      }),
    [query, kind],
  );

  return (
    <div className="page stack">
      <PageHeader
        title="Evidence"
        subtitle="Files are copied into a managed vault, hashed, and stored owner-only. AuditForge never executes or opens them."
        actions={
          <Button variant="primary" onClick={() => setImporting(true)}>
            Import file
          </Button>
        }
      />

      <div className="row">
        <input
          className="input"
          style={{ maxWidth: 340 }}
          placeholder="Filter by title, reference or filename"
          value={search}
          onChange={(event) => setSearch(event.target.value)}
          aria-label="Filter evidence"
        />
        <select
          className="select"
          style={{ maxWidth: 200 }}
          value={kind}
          onChange={(event) => setKind(event.target.value as EvidenceKind | "all")}
          aria-label="Filter by type"
        >
          {KIND_FILTERS.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
      </div>

      {evidence.error && <Notice tone="danger">{evidence.error.message}</Notice>}

      <Card flush>
        {evidence.loading && !evidence.data ? (
          <LoadingBlock />
        ) : !evidence.data || evidence.data.length === 0 ? (
          <EmptyState
            title="No evidence yet"
            hint="Import screenshots, logs, captures or command output. Each file's digest is recorded so a reader can verify it."
            action={
              <Button variant="primary" onClick={() => setImporting(true)}>
                Import a file
              </Button>
            }
          />
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>Reference</th>
                <th>Title</th>
                <th>File</th>
                <th>Type</th>
                <th className="right">Size</th>
                <th>Links</th>
                <th className="right">Added</th>
              </tr>
            </thead>
            <tbody>
              {evidence.data.map((item) => (
                <tr key={item.id} data-clickable="true" onClick={() => setInspecting(item)}>
                  <td className="table__ref">{item.evidence_ref}</td>
                  <td className="table__main table__truncate">
                    {item.title}
                    {item.is_encrypted && <span className="badge badge--accent"> Encrypted</span>}
                  </td>
                  <td className="muted table__truncate">{item.original_filename}</td>
                  <td className="muted nowrap tiny">{humanize(item.kind)}</td>
                  <td className="table__num muted nowrap">{formatBytes(item.byte_size)}</td>
                  <td className="muted nowrap tiny">{item.links.length}</td>
                  <td className="table__num muted nowrap tiny">{formatDateTime(item.created_at)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Card>

      {importing && (
        <ImportDialog
          onClose={() => setImporting(false)}
          onImported={() => {
            setImporting(false);
            evidence.reload();
          }}
        />
      )}

      {inspecting && (
        <EvidenceDialog
          evidence={inspecting}
          onClose={() => setInspecting(null)}
          onDeleted={() => {
            setInspecting(null);
            evidence.reload();
          }}
        />
      )}
    </div>
  );
}

function ImportDialog({ onClose, onImported }: { onClose: () => void; onImported: () => void }) {
  const toast = useToast();
  const settings = useAsync(() => api.getSettings(), []);
  const engagements = useAsync(() => api.listEngagements({ limit: 200 }), []);

  const [sourcePath, setSourcePath] = useState("");
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [engagementId, setEngagementId] = useState<string | null>(null);
  const [encrypt, setEncrypt] = useState(false);
  const [acknowledge, setAcknowledge] = useState(false);

  const [pick, pickState] = useAction(async () => {
    const chosen = await openFileDialog({ multiple: false, directory: false });
    if (typeof chosen !== "string") return null;
    setSourcePath(chosen);
    if (!title) {
      const name = chosen.split(/[\\/]/).pop() ?? "";
      setTitle(name);
    }
    return chosen;
  });

  const [runImport, importState] = useAction(async () =>
    api.importEvidence({
      source_path: sourcePath,
      title,
      description,
      engagement_id: engagementId,
      encrypt,
      acknowledge_executable: acknowledge,
    }),
  );

  const refused = importState.error?.message.includes("executable");

  return (
    <Modal
      title="Import evidence"
      onClose={onClose}
      wide
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button
            variant="primary"
            busy={importState.busy}
            disabled={!sourcePath || !title.trim()}
            onClick={async () => {
              const imported = await runImport();
              if (imported) {
                toast.success(`${imported.evidence_ref} imported`);
                onImported();
              }
            }}
          >
            Import
          </Button>
        </>
      }
    >
      <div className="stack">
        {importState.error && <Notice tone="danger">{importState.error.message}</Notice>}

        <div className="field">
          <span className="field__label">
            File<span className="field__required">*</span>
          </span>
          <div className="row">
            <input className="input" value={sourcePath} readOnly placeholder="No file chosen" />
            <Button busy={pickState.busy} onClick={() => void pick()}>
              Choose…
            </Button>
          </div>
          <div className="field__help">
            The file is copied into the vault. Your original is left where it is.
          </div>
        </div>

        <TextField label="Title" value={title} onValueChange={setTitle} required />
        <TextAreaField
          label="Description"
          value={description}
          onValueChange={setDescription}
          help="What this shows, and how it was captured."
        />

        <div className="field">
          <label className="field__label" htmlFor="evidence-engagement">
            Engagement
          </label>
          <select
            id="evidence-engagement"
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

        <Checkbox
          checked={encrypt || (settings.data?.security.encrypt_evidence_by_default ?? false)}
          onChange={setEncrypt}
          label="Encrypt this file at rest"
          hint="AES-256-GCM under a key held in the OS keychain. Recommended for credentials or personal data."
        />

        {refused && (
          <>
            <Notice tone="warning" title="This looks like an executable or a script">
              AuditForge refuses these by default. If you are deliberately keeping a sample, say so
              below. It is still never executed or opened, and the acknowledgement is recorded in
              the audit trail.
            </Notice>
            <Checkbox
              checked={acknowledge}
              onChange={setAcknowledge}
              label="I am importing an executable or script on purpose"
            />
          </>
        )}
      </div>
    </Modal>
  );
}

function EvidenceDialog({
  evidence,
  onClose,
  onDeleted,
}: {
  evidence: Evidence;
  onClose: () => void;
  onDeleted: () => void;
}) {
  const toast = useToast();
  const [confirmDelete, setConfirmDelete] = useState(false);

  const [reveal, revealState] = useAction(async () => {
    const path = await api.evidencePath(evidence.id);
    await openPath(path);
    return path;
  });

  const [remove, removeState] = useAction(async () => {
    await api.deleteEvidence(evidence.id);
    return true;
  });

  return (
    <Modal
      title={evidence.title}
      onClose={onClose}
      wide
      footer={
        <>
          <Button
            variant="danger"
            onClick={() => setConfirmDelete(true)}
            disabled={removeState.busy}
          >
            Delete
          </Button>
          <div className="grow" />
          <Button
            busy={revealState.busy}
            onClick={async () => {
              const path = await reveal();
              if (path) toast.push("Opened with your desktop's default handler.");
            }}
          >
            Open with the system handler
          </Button>
          <Button variant="primary" onClick={onClose}>
            Close
          </Button>
        </>
      }
    >
      <div className="stack">
        <KeyValues
          items={[
            ["Reference", <span className="mono">{evidence.evidence_ref}</span>],
            ["Original filename", evidence.original_filename],
            ["Stored as", <span className="mono">{evidence.relative_path}</span>],
            ["Type", `${humanize(evidence.kind)} (${evidence.mime_type})`],
            ["Size", formatBytes(evidence.byte_size)],
            ["SHA-256", <span className="mono">{evidence.sha256}</span>],
            ["Encrypted at rest", evidence.is_encrypted ? "Yes" : "No"],
            ["Captured", formatDateTime(evidence.captured_at)],
            ["Imported", formatDateTime(evidence.created_at)],
            [
              "Linked to",
              evidence.links.length === 0
                ? "Nothing yet"
                : evidence.links
                    .map((link) => `${humanize(link.target_type)} ${link.target_id.slice(0, 8)}`)
                    .join(", "),
            ],
          ]}
        />

        {evidence.description && <p className="prewrap small">{evidence.description}</p>}

        <Notice tone="neutral">
          The digest above is of the file as imported. AuditForge re-checks it whenever the file is
          read, so a modified copy in the vault is detected rather than quietly used.
        </Notice>

        {revealState.error && <Notice tone="danger">{revealState.error.message}</Notice>}

        {confirmDelete && (
          <Notice tone="danger" title="Delete this evidence?">
            <p className="small">
              The stored file and its record are removed. Reports already finalized keep their
              citation of it, including the digest.
            </p>
            <div className="row" style={{ marginTop: 8 }}>
              <Button size="sm" onClick={() => setConfirmDelete(false)}>
                Cancel
              </Button>
              <Button
                size="sm"
                variant="danger"
                busy={removeState.busy}
                onClick={async () => {
                  const done = await remove();
                  if (done) {
                    toast.success(`${evidence.evidence_ref} deleted`);
                    onDeleted();
                  }
                }}
              >
                Delete permanently
              </Button>
            </div>
            {removeState.error && <p className="tiny">{removeState.error.message}</p>}
          </Notice>
        )}
      </div>
    </Modal>
  );
}
