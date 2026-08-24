import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { AiSettings, AppSettings, StartupStatus, User } from "@/types";

const getSettings = vi.fn();
const saveAiSettings = vi.fn();
const aiStatus = vi.fn();

vi.mock("@/services/api", () => ({
  api: {
    startupStatus: () => Promise.resolve(startupStatus()),
    currentUser: () => Promise.resolve(user()),
    sessionRemaining: () => Promise.resolve(1800),
    getSettings: () => getSettings(),
    saveAiSettings: (value: AiSettings) => saveAiSettings(value),
    aiStatus: () => aiStatus(),
    listTemplates: () => Promise.resolve([]),
    auditLog: () => Promise.resolve([]),
    setApiKey: () => Promise.resolve(settings()),
    clearApiKey: () => Promise.resolve(settings()),
  },
  onSignedOut: () => () => undefined,
  BackendError: class extends Error {
    kind = "internal";
  },
}));

const { SettingsPage } = await import("./SettingsPage");
const { SessionProvider } = await import("@/features/auth/SessionProvider");
const { ToastProvider } = await import("@/components/Toast");

function user(): User {
  return {
    id: "u1",
    username: "analyst",
    full_name: "Spencer Rawlson Spady",
    job_title: "Cloud Computing & Cybersecurity",
    email: "",
    organization_id: "o1",
    organization_name: "PLH Ecosystem, Inc.",
    is_active: true,
    last_login_at: null,
    password_changed_at: "2026-08-24T09:00:00Z",
    created_at: "2026-08-24T09:00:00Z",
    updated_at: "2026-08-24T09:00:00Z",
  };
}

function startupStatus(): StartupStatus {
  return {
    has_account: true,
    signed_in: true,
    user: user(),
    version: "0.1.0",
    data_dir: "C:\\Users\\test\\AppData\\Roaming\\AuditForge",
    secret_backend: "OS keychain",
    secrets_os_backed: true,
  };
}

function ai(overrides: Partial<AiSettings> = {}): AiSettings {
  return {
    provider: "disabled",
    openai_model: "gpt-4.1-mini",
    openai_base_url: "https://api.openai.com/v1",
    local_endpoint: "http://127.0.0.1:11434",
    local_model: "llama3.1:8b",
    temperature: 0.2,
    max_output_tokens: 1500,
    request_timeout_secs: 90,
    cloud_consent: false,
    redact_before_send: true,
    has_api_key: false,
    ...overrides,
  };
}

function settings(overrides: Partial<AiSettings> = {}): AppSettings {
  return {
    security: {
      idle_timeout_secs: 1800,
      absolute_timeout_secs: 43200,
      encrypt_evidence_by_default: false,
      warn_on_risky_evidence: true,
    },
    ai: ai(overrides),
    reports: {
      default_template_key: "penetration_test",
      default_classification: "confidential",
      pdf: {
        page_size: "a4",
        include_table_of_contents: true,
        include_evidence_appendix: true,
        include_evidence_images: true,
        show_provenance_markers: false,
        footer_note: "",
      },
    },
    ui: { theme: "system", compact_density: false },
  };
}

async function openAiTab() {
  const person = userEvent.setup();
  render(
    <ToastProvider>
      <SessionProvider>
        <SettingsPage />
      </SessionProvider>
    </ToastProvider>,
  );
  await waitFor(() => expect(screen.getByRole("tab", { name: "AI" })).toBeInTheDocument());
  await person.click(screen.getByRole("tab", { name: "AI" }));
  await waitFor(() => expect(screen.getByText("Where your text goes")).toBeInTheDocument());
  return person;
}

describe("the AI settings tab", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getSettings.mockResolvedValue(settings());
    saveAiSettings.mockImplementation((value: AiSettings) =>
      Promise.resolve({ ...settings(), ai: { ...value, has_api_key: true } }),
    );
    aiStatus.mockResolvedValue({
      provider: "disabled",
      privacy_mode: "offline",
      banner: "AI Processing: Disabled / Offline",
      destination: "No AI provider is configured.",
      model: "",
      enabled: false,
      leaves_machine: false,
      redaction_enabled: true,
    });
  });

  it("shows nothing to save until something is changed", async () => {
    await openAiTab();
    expect(screen.queryByText("You have unsaved changes.")).not.toBeInTheDocument();
  });

  it("surfaces a save bar as soon as a provider is chosen", async () => {
    const person = await openAiTab();

    // This is the regression: choosing a provider only edits the form, and the previous
    // Save button sat at the bottom of a different card where it was easy to miss.
    await person.click(screen.getByRole("button", { name: /OpenAI/ }));

    expect(await screen.findByText("You have unsaved changes.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save AI settings" })).toBeInTheDocument();
    expect(screen.getByText("Not applied yet")).toBeInTheDocument();
    expect(saveAiSettings).not.toHaveBeenCalled();
  });

  it("persists the provider and consent together when saved", async () => {
    const person = await openAiTab();

    await person.click(screen.getByRole("button", { name: /OpenAI/ }));
    await person.click(
      screen.getByLabelText(/I consent to sending report text/, { selector: "input" }),
    );
    await person.click(screen.getByRole("button", { name: "Save AI settings" }));

    await waitFor(() => expect(saveAiSettings).toHaveBeenCalledTimes(1));
    const saved = saveAiSettings.mock.calls[0]![0] as AiSettings;
    expect(saved.provider).toBe("openai");
    expect(saved.cloud_consent).toBe(true);
  });

  it("discards a pending change on request", async () => {
    const person = await openAiTab();

    await person.click(screen.getByRole("button", { name: /OpenAI/ }));
    expect(await screen.findByText("You have unsaved changes.")).toBeInTheDocument();

    await person.click(screen.getByRole("button", { name: "Discard" }));
    await waitFor(() =>
      expect(screen.queryByText("You have unsaved changes.")).not.toBeInTheDocument(),
    );
    expect(saveAiSettings).not.toHaveBeenCalled();
  });

  it("explains which gate is closed when a key exists but consent does not", async () => {
    getSettings.mockResolvedValue(settings({ provider: "openai", has_api_key: true }));
    await openAiTab();

    expect(await screen.findByText("Selected, but not active yet")).toBeInTheDocument();
    expect(screen.getByText(/An API key is not enough on its own/)).toBeInTheDocument();
  });
});
