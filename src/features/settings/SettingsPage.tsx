/**
 * Settings: profile, security, AI and report defaults, plus the audit log.
 *
 * The AI tab is where the privacy decision is made, so it states plainly what each mode
 * does and requires an explicit consent toggle before anything can reach a cloud
 * provider. The API key is write-only from here: it goes to the OS keychain and is never
 * read back.
 */

import { useEffect, useState } from "react";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";

import { PrivacyBadge } from "@/components/badges";
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
  SelectField,
  Tabs,
  TextAreaField,
  TextField,
} from "@/components/ui";
import { useSession } from "@/features/auth/SessionProvider";
import { useAction, useAsync } from "@/hooks/useAsync";
import { api } from "@/services/api";
import { formatDateTime, humanize } from "@/services/format";
import type {
  AiProviderKind,
  AiSettings,
  Classification,
  ReportSettings,
  SecuritySettings,
  UiSettings,
} from "@/types";

type Tab = "profile" | "security" | "ai" | "reports" | "audit";

export function SettingsPage() {
  const [tab, setTab] = useState<Tab>("profile");
  const { settings, status } = useSession();

  return (
    <div className="page stack">
      <PageHeader
        title="Settings"
        subtitle={
          status ? (
            <span className="row row--wrap">
              <span>AuditForge {status.version}</span>
              <span className="mono tiny">{status.data_dir}</span>
            </span>
          ) : undefined
        }
      />

      <Tabs
        active={tab}
        onChange={setTab}
        tabs={[
          { id: "profile", label: "Profile" },
          { id: "security", label: "Security" },
          { id: "ai", label: "AI" },
          { id: "reports", label: "Reports" },
          { id: "audit", label: "Audit log" },
        ]}
      />

      {!settings && tab !== "profile" ? (
        <LoadingBlock />
      ) : (
        <>
          {tab === "profile" && <ProfileTab />}
          {tab === "security" && settings && <SecurityTab value={settings.security} />}
          {tab === "ai" && settings && <AiTab value={settings.ai} />}
          {tab === "reports" && settings && (
            <ReportsTab value={settings.reports} ui={settings.ui} />
          )}
          {tab === "audit" && <AuditTab />}
        </>
      )}
    </div>
  );
}

// ---------------------------------------------------------------- profile

function ProfileTab() {
  const toast = useToast();
  const { user, refreshUser } = useSession();
  const [fullName, setFullName] = useState(user?.full_name ?? "");
  const [organization, setOrganization] = useState(user?.organization_name ?? "");
  const [jobTitle, setJobTitle] = useState(user?.job_title ?? "");
  const [email, setEmail] = useState(user?.email ?? "");

  const [save, saveState] = useAction(async () => {
    await api.updateProfile({
      full_name: fullName,
      organization,
      job_title: jobTitle,
      email,
    });
    await refreshUser();
    return true;
  });

  return (
    <div className="grid grid--2">
      <Card title="Author details">
        <div className="stack">
          <Notice tone="info">
            These become the author on every new report. Reports you have already finalized keep
            the details they were signed off with; changing your title today cannot rewrite a
            document issued last year.
          </Notice>
          {saveState.error && <Notice tone="danger">{saveState.error.message}</Notice>}
          <TextField label="Full name" value={fullName} onValueChange={setFullName} required />
          <TextField
            label="Company or organization"
            value={organization}
            onValueChange={setOrganization}
          />
          <TextField
            label="Job title or credentials"
            value={jobTitle}
            onValueChange={setJobTitle}
            help="Printed under your name on the cover page."
          />
          <TextField label="Email" type="email" value={email} onValueChange={setEmail} />
          <div>
            <Button
              variant="primary"
              busy={saveState.busy}
              onClick={() => void save().then((ok) => ok && toast.success("Profile updated"))}
            >
              Save profile
            </Button>
          </div>
        </div>
      </Card>

      <Card title="How you will appear">
        <div className="stack">
          <div className="card" style={{ padding: 18, background: "var(--surface-sunken)" }}>
            <div className="compare__label">Prepared by</div>
            <div className="strong">{fullName || "—"}</div>
            <div className="compare__label" style={{ marginTop: 10 }}>
              Organization
            </div>
            <div>{organization || "—"}</div>
            <div className="compare__label" style={{ marginTop: 10 }}>
              Role
            </div>
            <div>{jobTitle || "—"}</div>
          </div>
          <KeyValues
            items={[
              ["Username", user?.username ?? "—"],
              ["Account created", formatDateTime(user?.created_at)],
              ["Last sign-in", formatDateTime(user?.last_login_at)],
              ["Password changed", formatDateTime(user?.password_changed_at)],
            ]}
          />
        </div>
      </Card>
    </div>
  );
}

// ---------------------------------------------------------------- security

function SecurityTab({ value }: { value: SecuritySettings }) {
  const toast = useToast();
  const { applySettings } = useSession();
  const [form, setForm] = useState(value);
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [confirm, setConfirm] = useState("");

  const [save, saveState] = useAction(async () => {
    applySettings(await api.saveSecuritySettings(form));
    return true;
  });

  const [changePassword, passwordState] = useAction(async () => {
    await api.changePassword(current, next, confirm);
    return true;
  });

  const [backup, backupState] = useAction(async () => {
    const destination = await saveDialog({ defaultPath: "auditforge-backup.db" });
    if (!destination) return null;
    return api.backupDatabase(destination);
  });

  return (
    <div className="grid grid--2">
      <Card title="Session">
        <div className="stack">
          {saveState.error && <Notice tone="danger">{saveState.error.message}</Notice>}
          <SelectField
            label="Sign out after inactivity"
            value={String(form.idle_timeout_secs)}
            options={[
              { value: "300", label: "5 minutes" },
              { value: "900", label: "15 minutes" },
              { value: "1800", label: "30 minutes" },
              { value: "3600", label: "1 hour" },
              { value: "14400", label: "4 hours" },
            ]}
            onValueChange={(seconds) =>
              setForm({ ...form, idle_timeout_secs: Number(seconds) })
            }
          />
          <SelectField
            label="Maximum session length"
            value={String(form.absolute_timeout_secs)}
            help="Applies even while you are active."
            options={[
              { value: "14400", label: "4 hours" },
              { value: "43200", label: "12 hours" },
              { value: "86400", label: "24 hours" },
            ]}
            onValueChange={(seconds) =>
              setForm({ ...form, absolute_timeout_secs: Number(seconds) })
            }
          />
          <Checkbox
            checked={form.encrypt_evidence_by_default}
            onChange={(checked) => setForm({ ...form, encrypt_evidence_by_default: checked })}
            label="Encrypt new evidence at rest by default"
            hint="AES-256-GCM under a key held in the OS keychain."
          />
          <Checkbox
            checked={form.warn_on_risky_evidence}
            onChange={(checked) => setForm({ ...form, warn_on_risky_evidence: checked })}
            label="Warn before importing executables or scripts"
            hint="AuditForge never runs evidence; this controls the warning only."
          />
          <div>
            <Button
              variant="primary"
              busy={saveState.busy}
              onClick={() => void save().then((ok) => ok && toast.success("Security settings saved"))}
            >
              Save
            </Button>
          </div>
        </div>
      </Card>

      <Card title="Password">
        <div className="stack">
          {passwordState.error && <Notice tone="danger">{passwordState.error.message}</Notice>}
          <Notice tone="neutral">
            Changing your password signs out every session, including this one.
          </Notice>
          <TextField
            label="Current password"
            type="password"
            value={current}
            onValueChange={setCurrent}
            autoComplete="current-password"
          />
          <TextField
            label="New password"
            type="password"
            value={next}
            onValueChange={setNext}
            autoComplete="new-password"
          />
          <TextField
            label="Confirm new password"
            type="password"
            value={confirm}
            onValueChange={setConfirm}
            autoComplete="new-password"
          />
          <div>
            <Button
              busy={passwordState.busy}
              disabled={!current || !next || next !== confirm}
              onClick={() => void changePassword()}
            >
              Change password
            </Button>
          </div>
        </div>
      </Card>

      <Card title="Backup">
        <div className="stack">
          <p className="small muted">
            Writes a consistent copy of the database, safe to run while you are working. It
            contains report content and password hashes, but not evidence files or secrets: back
            up the evidence folder separately.
          </p>
          {backupState.error && <Notice tone="danger">{backupState.error.message}</Notice>}
          <div>
            <Button
              busy={backupState.busy}
              onClick={async () => {
                const path = await backup();
                if (path) toast.success(`Backup written to ${path}`);
              }}
            >
              Back up database
            </Button>
          </div>
        </div>
      </Card>
    </div>
  );
}

// ---------------------------------------------------------------- AI

const PROVIDERS: Array<{ value: AiProviderKind; label: string; blurb: string }> = [
  {
    value: "disabled",
    label: "Disabled",
    blurb: "No AI features. Everything else in AuditForge works exactly the same.",
  },
  {
    value: "local",
    label: "Local AI",
    blurb: "Ollama or a compatible endpoint on this machine. Text does not leave the device.",
  },
  {
    value: "openai",
    label: "OpenAI",
    blurb: "The OpenAI API. Report text is sent to a third party.",
  },
];

function AiTab({ value }: { value: AiSettings }) {
  const toast = useToast();
  const { applySettings } = useSession();
  const [form, setForm] = useState(value);
  const [apiKey, setApiKey] = useState("");
  const status = useAsync(() => api.aiStatus(), [value]);

  useEffect(() => setForm(value), [value]);

  const [save, saveState] = useAction(async () => {
    applySettings(await api.saveAiSettings(form));
    return true;
  });

  const [storeKey, keyState] = useAction(async () => {
    applySettings(await api.setApiKey(apiKey));
    setApiKey("");
    return true;
  });

  const [clearKey, clearState] = useAction(async () => {
    applySettings(await api.clearApiKey());
    return true;
  });

  return (
    <div className="stack">
      <Card title="Where your text goes">
        <div className="stack">
          {status.data && (
            <div className="row row--wrap">
              <PrivacyBadge mode={status.data.privacy_mode} />
              <span className="small muted">{status.data.destination}</span>
            </div>
          )}
          <div className="grid grid--3">
            {PROVIDERS.map((provider) => (
              <button
                key={provider.value}
                type="button"
                className="card"
                aria-pressed={form.provider === provider.value}
                style={{
                  padding: "12px 14px",
                  textAlign: "left",
                  cursor: "pointer",
                  borderColor: form.provider === provider.value ? "var(--accent)" : undefined,
                  background: form.provider === provider.value ? "var(--accent-soft)" : undefined,
                }}
                onClick={() => setForm({ ...form, provider: provider.value })}
              >
                <div className="strong small">{provider.label}</div>
                <div className="tiny muted" style={{ marginTop: 3 }}>
                  {provider.blurb}
                </div>
              </button>
            ))}
          </div>

          {form.provider === "openai" && (
            <Notice tone="warning" title="Cloud processing sends report text to OpenAI">
              Only turn this on for material you are permitted to share with a third party. Client
              data, unpublished findings and anything under NDA usually is not. Local AI gives you
              the same features without the text leaving this machine.
            </Notice>
          )}

          <Checkbox
            checked={form.cloud_consent}
            onChange={(checked) => setForm({ ...form, cloud_consent: checked })}
            label="I consent to sending report text to the configured cloud provider"
            hint="Required in addition to selecting OpenAI. Without it, cloud calls are refused."
          />

          <Checkbox
            checked={form.redact_before_send}
            onChange={(checked) => setForm({ ...form, redact_before_send: checked })}
            label="Redact structured secrets before sending"
            hint="Replaces IP addresses, emails, access key ids, tokens and private key blocks with placeholders, and restores the real values in the result. It does not cover hostnames or names."
          />
        </div>
      </Card>

      <div className="grid grid--2">
        <Card title="Local AI">
          <div className="stack">
            <TextField
              label="Endpoint"
              value={form.local_endpoint}
              onValueChange={(next) => setForm({ ...form, local_endpoint: next })}
              help="Ollama's default is http://127.0.0.1:11434"
            />
            <TextField
              label="Model"
              value={form.local_model}
              onValueChange={(next) => setForm({ ...form, local_model: next })}
              help="Must already be pulled, e.g. `ollama pull llama3.1:8b`"
            />
          </div>
        </Card>

        <Card title="OpenAI">
          <div className="stack">
            <TextField
              label="Model"
              value={form.openai_model}
              onValueChange={(next) => setForm({ ...form, openai_model: next })}
            />
            <TextField
              label="Base URL"
              value={form.openai_base_url}
              onValueChange={(next) => setForm({ ...form, openai_base_url: next })}
              help="Must use https."
            />

            <div className="field">
              <span className="field__label">API key</span>
              <div className="field__help">
                Stored in your operating system's keychain, never in the database and never shown
                again after you save it.
              </div>
              {value.has_api_key ? (
                <div className="row" style={{ marginTop: 6 }}>
                  <span className="badge badge--success">A key is stored</span>
                  <Button
                    size="sm"
                    variant="danger"
                    busy={clearState.busy}
                    onClick={() => void clearKey().then((ok) => ok && toast.success("API key removed"))}
                  >
                    Remove key
                  </Button>
                </div>
              ) : (
                <div className="row" style={{ marginTop: 6 }}>
                  <input
                    className="input"
                    type="password"
                    value={apiKey}
                    placeholder="sk-…"
                    onChange={(event) => setApiKey(event.target.value)}
                    aria-label="OpenAI API key"
                  />
                  <Button
                    busy={keyState.busy}
                    disabled={!apiKey.trim()}
                    onClick={() => void storeKey().then((ok) => ok && toast.success("API key stored"))}
                  >
                    Save key
                  </Button>
                </div>
              )}
              {(keyState.error || clearState.error) && (
                <div className="field__error">{(keyState.error ?? clearState.error)?.message}</div>
              )}
            </div>
          </div>
        </Card>
      </div>

      <Card title="Generation">
        <div className="grid grid--3">
          <TextField
            label="Temperature"
            type="number"
            min={0}
            max={2}
            step={0.1}
            value={form.temperature}
            onValueChange={(next) => setForm({ ...form, temperature: Number(next) })}
            help="Lower keeps the assistant closer to your wording."
          />
          <TextField
            label="Maximum output tokens"
            type="number"
            min={1}
            max={32000}
            value={form.max_output_tokens}
            onValueChange={(next) => setForm({ ...form, max_output_tokens: Number(next) })}
          />
          <TextField
            label="Request timeout (seconds)"
            type="number"
            min={5}
            max={600}
            value={form.request_timeout_secs}
            onValueChange={(next) => setForm({ ...form, request_timeout_secs: Number(next) })}
            help="Large local models may need more."
          />
        </div>
        {saveState.error && <Notice tone="danger">{saveState.error.message}</Notice>}
        <div style={{ marginTop: 14 }}>
          <Button
            variant="primary"
            busy={saveState.busy}
            onClick={() => void save().then((ok) => ok && toast.success("AI settings saved"))}
          >
            Save AI settings
          </Button>
        </div>
      </Card>

      <Notice tone="neutral" title="What the assistant will not do">
        It does not set severity, invent IP addresses, hostnames, CVEs, CVSS scores, timestamps or
        remediation steps, and it does not add facts your text does not contain. Suggestions that
        introduce new identifiers are refused before you see them, and every suggestion is recorded
        with what you decided about it.
      </Notice>
    </div>
  );
}

// ---------------------------------------------------------------- reports

function ReportsTab({ value, ui }: { value: ReportSettings; ui: UiSettings }) {
  const toast = useToast();
  const { applySettings } = useSession();
  const [form, setForm] = useState(value);
  const [uiForm, setUiForm] = useState(ui);
  const templates = useAsync(() => api.listTemplates(), []);

  const [save, saveState] = useAction(async () => {
    applySettings(await api.saveReportSettings(form));
    applySettings(await api.saveUiSettings(uiForm));
    return true;
  });

  return (
    <div className="grid grid--2">
      <Card title="Report defaults">
        <div className="stack">
          <SelectField
            label="Default report type"
            value={form.default_template_key}
            options={(templates.data ?? []).map((template) => ({
              value: template.key,
              label: template.name,
            }))}
            onValueChange={(key) => setForm({ ...form, default_template_key: key })}
          />
          <SelectField
            label="Default classification"
            value={form.default_classification}
            options={[
              { value: "public", label: "Public" },
              { value: "internal", label: "Internal" },
              { value: "confidential", label: "Confidential" },
              { value: "restricted", label: "Restricted" },
            ]}
            onValueChange={(next) =>
              setForm({ ...form, default_classification: next as Classification })
            }
          />
        </div>
      </Card>

      <Card title="PDF output">
        <div className="stack">
          <SelectField
            label="Page size"
            value={form.pdf.page_size}
            options={[
              { value: "a4", label: "A4" },
              { value: "letter", label: "US Letter" },
            ]}
            onValueChange={(size) =>
              setForm({ ...form, pdf: { ...form.pdf, page_size: size as "a4" | "letter" } })
            }
          />
          <Checkbox
            checked={form.pdf.include_table_of_contents}
            onChange={(checked) =>
              setForm({ ...form, pdf: { ...form.pdf, include_table_of_contents: checked } })
            }
            label="Include a table of contents"
          />
          <Checkbox
            checked={form.pdf.include_evidence_appendix}
            onChange={(checked) =>
              setForm({ ...form, pdf: { ...form.pdf, include_evidence_appendix: checked } })
            }
            label="Include the evidence appendix"
          />
          <Checkbox
            checked={form.pdf.include_evidence_images}
            onChange={(checked) =>
              setForm({ ...form, pdf: { ...form.pdf, include_evidence_images: checked } })
            }
            label="Embed image evidence"
            hint="Off keeps documents small and avoids re-publishing screenshots."
          />
          <Checkbox
            checked={form.pdf.show_provenance_markers}
            onChange={(checked) =>
              setForm({ ...form, pdf: { ...form.pdf, show_provenance_markers: checked } })
            }
            label="Print a marker beside AI-assisted text"
            hint="The statement of AI assistance is always included when any assisted text is present."
          />
          <TextAreaField
            label="Footer note"
            value={form.pdf.footer_note}
            onValueChange={(note) => setForm({ ...form, pdf: { ...form.pdf, footer_note: note } })}
            help="Appears beside the date in the page footer."
          />
        </div>
      </Card>

      <Card title="Appearance">
        <div className="stack">
          <SelectField
            label="Theme"
            value={uiForm.theme}
            options={[
              { value: "system", label: "Match the system" },
              { value: "light", label: "Light" },
              { value: "dark", label: "Dark" },
            ]}
            onValueChange={(theme) =>
              setUiForm({ ...uiForm, theme: theme as UiSettings["theme"] })
            }
          />
          <Checkbox
            checked={uiForm.compact_density}
            onChange={(checked) => setUiForm({ ...uiForm, compact_density: checked })}
            label="Compact rows"
            hint="Fits more into lists at a slightly smaller size."
          />
          {saveState.error && <Notice tone="danger">{saveState.error.message}</Notice>}
          <div>
            <Button
              variant="primary"
              busy={saveState.busy}
              onClick={() => void save().then((ok) => ok && toast.success("Settings saved"))}
            >
              Save
            </Button>
          </div>
        </div>
      </Card>
    </div>
  );
}

// ---------------------------------------------------------------- audit

function AuditTab() {
  const [action, setAction] = useState("");
  const entries = useAsync(
    () => api.auditLog({ limit: 300, ...(action ? { action } : {}) }),
    [action],
  );

  return (
    <div className="stack">
      <Notice tone="neutral">
        Every significant action is recorded here: sign-ins, edits, severity decisions, evidence
        imports, AI suggestions and what you decided about them, finalizations and exports. The log
        is append-only; AuditForge offers no way to edit or delete an entry.
      </Notice>

      <div className="row">
        <input
          className="input"
          style={{ maxWidth: 300 }}
          placeholder="Filter by action, e.g. report_finalized"
          value={action}
          onChange={(event) => setAction(event.target.value.trim())}
          aria-label="Filter by action"
        />
      </div>

      {entries.error && <Notice tone="danger">{entries.error.message}</Notice>}

      <Card flush>
        {entries.loading && !entries.data ? (
          <LoadingBlock />
        ) : !entries.data || entries.data.length === 0 ? (
          <EmptyState title="No matching entries" />
        ) : (
          <table className="table">
            <thead>
              <tr>
                <th>When</th>
                <th>Who</th>
                <th>Action</th>
                <th>Target</th>
                <th>Outcome</th>
                <th>Detail</th>
              </tr>
            </thead>
            <tbody>
              {entries.data.map((entry) => (
                <tr key={entry.id}>
                  <td className="muted nowrap tiny">{formatDateTime(entry.occurred_at)}</td>
                  <td className="muted nowrap">{entry.username || "—"}</td>
                  <td className="nowrap">{humanize(entry.action)}</td>
                  <td className="muted nowrap tiny">
                    {entry.target_type ? `${entry.target_type} ${entry.target_id.slice(0, 8)}` : "—"}
                  </td>
                  <td>
                    <span
                      className={`badge badge--${
                        entry.outcome === "success"
                          ? "success"
                          : entry.outcome === "denied"
                            ? "warning"
                            : "danger"
                      }`}
                    >
                      {entry.outcome}
                    </span>
                  </td>
                  <td className="mono tiny table__truncate">
                    {Object.keys(entry.detail).length > 0 ? JSON.stringify(entry.detail) : ""}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Card>
    </div>
  );
}
