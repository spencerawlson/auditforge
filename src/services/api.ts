/**
 * The single boundary between the UI and the Rust core.
 *
 * Every backend call goes through `call`, which gives the whole frontend one place that
 * knows about `invoke`, one error shape, and one set of type assertions. Nothing else in
 * `src/` imports from `@tauri-apps/api`.
 */

import { invoke } from "@tauri-apps/api/core";

import type {
  AiReviewResult,
  AiRequestRow,
  AiSettings,
  AiStatus,
  AiSuggestion,
  AiTextRequest,
  AppSettings,
  AuditEntry,
  AuditQuery,
  Completeness,
  CorrectiveAction,
  CorrectiveActionUpdate,
  Dashboard,
  Engagement,
  EngagementFilter,
  EngagementSummary,
  EngagementUpdate,
  Evidence,
  EvidenceFilter,
  EvidenceLinkRequest,
  EvidenceUpdate,
  ExportFormat,
  Finding,
  FindingFilter,
  FindingUpdate,
  GuidedQuestion,
  Incident,
  IncidentFilter,
  IncidentUpdate,
  NewCorrectiveAction,
  NewEngagement,
  NewEvidence,
  NewFinding,
  NewIncident,
  NewPostmortem,
  NewReport,
  NewUser,
  Postmortem,
  PostmortemCompletion,
  PostmortemUpdate,
  ProfileUpdate,
  RedactionPreview,
  Report,
  ReportDraftUpdate,
  ReportFilter,
  ReportReadiness,
  ReportSettings,
  ReportSummary,
  ReportTemplate,
  ReportVersion,
  ReportVersionSummary,
  ReportWorkspace,
  SearchHit,
  SearchQuery,
  SecuritySettings,
  SeverityDecision,
  SignedIn,
  StartupStatus,
  TemplateSummary,
  UiSettings,
  User,
  ApiError,
} from "@/types";

/** An error raised by a backend command, carrying the machine-readable kind. */
export class BackendError extends Error {
  readonly kind: ApiError["kind"];
  readonly retryAfterSecs?: number;

  constructor(payload: ApiError) {
    super(payload.message);
    this.name = "BackendError";
    this.kind = payload.kind;
    if (payload.retry_after_secs !== undefined) {
      this.retryAfterSecs = payload.retry_after_secs;
    }
  }

  /** True when the session has ended and the UI should return to the login screen. */
  get isSignedOut(): boolean {
    return this.kind === "unauthorized";
  }
}

function toBackendError(error: unknown): BackendError {
  if (error && typeof error === "object" && "kind" in error && "message" in error) {
    return new BackendError(error as ApiError);
  }
  return new BackendError({
    kind: "internal",
    message: typeof error === "string" ? error : "The operation could not be completed.",
  });
}

/** Listeners notified when any command reports that the session has ended. */
const signOutListeners = new Set<() => void>();

export function onSignedOut(listener: () => void): () => void {
  signOutListeners.add(listener);
  return () => signOutListeners.delete(listener);
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return (await invoke(command, args)) as T;
  } catch (raw) {
    const error = toBackendError(raw);
    // A dead session is a global event, not a per-screen error: tell the shell once and
    // let it move the user to the login screen.
    if (error.isSignedOut && command !== "startup_status") {
      signOutListeners.forEach((listener) => listener());
    }
    throw error;
  }
}

export const api = {
  // ------------------------------------------------------------ account
  startupStatus: () => call<StartupStatus>("startup_status"),
  register: (input: NewUser) => call<User>("register", { input }),
  login: (username: string, password: string) =>
    call<SignedIn>("login", { username, password }),
  logout: () => call<void>("logout"),
  currentUser: () => call<User>("current_user"),
  sessionRemaining: () => call<number | null>("session_remaining"),
  updateProfile: (update: ProfileUpdate) => call<User>("update_profile", { update }),
  changePassword: (currentPassword: string, newPassword: string, confirmPassword: string) =>
    call<void>("change_password", {
      currentPassword,
      newPassword,
      confirmPassword,
    }),
  passwordStrength: (password: string) => call<number>("password_strength", { password }),

  // ------------------------------------------------------------ dashboard and search
  dashboard: () => call<Dashboard>("dashboard"),
  search: (query: SearchQuery) => call<SearchHit[]>("global_search", { query }),
  listTags: () => call<string[]>("list_tags"),
  tagsFor: (entityType: string, entityId: string) =>
    call<string[]>("tags_for", { entityType, entityId }),
  tagEntity: (entityType: string, entityId: string, tag: string) =>
    call<string[]>("tag_entity", { entityType, entityId, tag }),
  untagEntity: (entityType: string, entityId: string, tag: string) =>
    call<string[]>("untag_entity", { entityType, entityId, tag }),
  auditLog: (query: AuditQuery) => call<AuditEntry[]>("audit_log", { query }),

  // ------------------------------------------------------------ engagements
  listEngagements: (filter: EngagementFilter = {}) =>
    call<EngagementSummary[]>("list_engagements", { filter }),
  getEngagement: (id: string) => call<Engagement>("get_engagement", { id }),
  createEngagement: (input: NewEngagement) => call<Engagement>("create_engagement", { input }),
  updateEngagement: (id: string, update: EngagementUpdate) =>
    call<Engagement>("update_engagement", { id, update }),
  deleteEngagement: (id: string) => call<void>("delete_engagement", { id }),

  // ------------------------------------------------------------ findings
  listFindings: (filter: FindingFilter = {}) => call<Finding[]>("list_findings", { filter }),
  getFinding: (id: string) => call<Finding>("get_finding", { id }),
  createFinding: (input: NewFinding) => call<Finding>("create_finding", { input }),
  updateFinding: (id: string, update: FindingUpdate) =>
    call<Finding>("update_finding", { id, update }),
  setFindingSeverity: (id: string, decision: SeverityDecision) =>
    call<Finding>("set_finding_severity", { id, decision }),
  deleteFinding: (id: string) => call<void>("delete_finding", { id }),

  // ------------------------------------------------------------ evidence
  listEvidence: (filter: EvidenceFilter = {}) => call<Evidence[]>("list_evidence", { filter }),
  importEvidence: (input: NewEvidence) => call<Evidence>("import_evidence", { input }),
  updateEvidence: (id: string, update: EvidenceUpdate) =>
    call<Evidence>("update_evidence", { id, update }),
  linkEvidence: (id: string, request: EvidenceLinkRequest) =>
    call<Evidence>("link_evidence", { id, request }),
  unlinkEvidence: (id: string, request: EvidenceLinkRequest) =>
    call<Evidence>("unlink_evidence", { id, request }),
  deleteEvidence: (id: string) => call<void>("delete_evidence", { id }),
  evidencePath: (id: string) => call<string>("evidence_path", { id }),

  // ------------------------------------------------------------ incidents
  listIncidents: (filter: IncidentFilter = {}) => call<Incident[]>("list_incidents", { filter }),
  getIncident: (id: string) => call<Incident>("get_incident", { id }),
  createIncident: (input: NewIncident) => call<Incident>("create_incident", { input }),
  updateIncident: (id: string, update: IncidentUpdate) =>
    call<Incident>("update_incident", { id, update }),
  setIncidentSeverity: (id: string, decision: SeverityDecision) =>
    call<Incident>("set_incident_severity", { id, decision }),

  // ------------------------------------------------------------ postmortems
  postmortemQuestions: () => call<GuidedQuestion[]>("postmortem_questions"),
  listPostmortems: (limit?: number) => call<Postmortem[]>("list_postmortems", { limit }),
  getPostmortem: (id: string) => call<Postmortem>("get_postmortem", { id }),
  postmortemCompletion: (id: string) => call<PostmortemCompletion>("postmortem_completion", { id }),
  createPostmortem: (input: NewPostmortem) => call<Postmortem>("create_postmortem", { input }),
  updatePostmortem: (id: string, update: PostmortemUpdate) =>
    call<Postmortem>("update_postmortem", { id, update }),
  addCorrectiveAction: (input: NewCorrectiveAction) =>
    call<CorrectiveAction>("add_corrective_action", { input }),
  updateCorrectiveAction: (id: string, update: CorrectiveActionUpdate) =>
    call<CorrectiveAction>("update_corrective_action", { id, update }),
  postmortemCompleteness: (id: string) => call<Completeness>("postmortem_completeness", { id }),
  renderPostmortemPdf: (id: string) => call<string>("render_postmortem_pdf", { id }),
  postmortemExportFilename: (id: string, format: ExportFormat) =>
    call<string>("postmortem_export_filename", { id, format }),
  exportPostmortemTo: (id: string, format: ExportFormat, destination: string) =>
    call<string>("export_postmortem_to", { id, format, destination }),

  // ------------------------------------------------------------ reports
  listTemplates: () => call<TemplateSummary[]>("list_templates"),
  getTemplate: (key: string) => call<ReportTemplate>("get_template", { key }),
  listReports: (filter: ReportFilter = {}) => call<ReportSummary[]>("list_reports", { filter }),
  openReport: (id: string, versionId?: string) =>
    call<ReportWorkspace>("open_report", { id, versionId: versionId ?? null }),
  createReport: (input: NewReport) => call<Report>("create_report", { input }),
  saveReportDraft: (id: string, update: ReportDraftUpdate) =>
    call<ReportVersion>("save_report_draft", { id, update }),
  snapshotReport: (id: string, notes: string) =>
    call<ReportVersion>("snapshot_report", { id, notes }),
  reportReadiness: (id: string) => call<ReportReadiness>("report_readiness", { id }),
  submitReportForReview: (id: string) => call<ReportVersion>("submit_report_for_review", { id }),
  finalizeReport: (id: string, confirmed: boolean, notes: string) =>
    call<ReportVersion>("finalize_report", { id, confirmed, notes }),
  createReportRevision: (id: string) => call<ReportVersion>("create_report_revision", { id }),
  reportVersions: (id: string) => call<ReportVersionSummary[]>("report_versions", { id }),
  deleteReport: (id: string) => call<void>("delete_report", { id }),

  // ------------------------------------------------------------ preview and export
  previewReportHtml: (id: string, versionId?: string) =>
    call<string>("preview_report_html", { id, versionId: versionId ?? null }),
  renderPdfForViewing: (id: string, versionId?: string) =>
    call<string>("render_pdf_for_viewing", { id, versionId: versionId ?? null }),
  exportFilename: (id: string, format: ExportFormat, versionId?: string) =>
    call<string>("export_filename", { id, format, versionId: versionId ?? null }),
  exportReport: (id: string, format: ExportFormat, versionId?: string) =>
    call<string>("export_report", { id, format, versionId: versionId ?? null }),
  exportReportTo: (id: string, format: ExportFormat, destination: string, versionId?: string) =>
    call<string>("export_report_to", {
      id,
      format,
      destination,
      versionId: versionId ?? null,
    }),

  // ------------------------------------------------------------ AI
  aiStatus: () => call<AiStatus>("ai_status"),
  aiRedactionPreview: (text: string) => call<RedactionPreview>("ai_redaction_preview", { text }),
  aiSuggest: (request: AiTextRequest) => call<AiSuggestion>("ai_suggest", { request }),
  aiReviewReport: (reportId: string, versionId?: string) =>
    call<AiReviewResult>("ai_review_report", { reportId, versionId: versionId ?? null }),
  aiReviewPreview: (reportId: string, versionId?: string) =>
    call<string>("ai_review_preview", { reportId, versionId: versionId ?? null }),
  aiRecordDecision: (requestId: string, decision: "accepted" | "edited" | "rejected") =>
    call<void>("ai_record_decision", { requestId, decision }),
  aiRequestsFor: (targetType: string, targetId: string, limit?: number) =>
    call<AiRequestRow[]>("ai_requests_for", { targetType, targetId, limit }),

  // ------------------------------------------------------------ settings
  getSettings: () => call<AppSettings>("get_settings"),
  saveSecuritySettings: (value: SecuritySettings) =>
    call<AppSettings>("save_security_settings", { value }),
  saveAiSettings: (value: AiSettings) => call<AppSettings>("save_ai_settings", { value }),
  saveReportSettings: (value: ReportSettings) =>
    call<AppSettings>("save_report_settings", { value }),
  saveUiSettings: (value: UiSettings) => call<AppSettings>("save_ui_settings", { value }),
  setApiKey: (key: string) => call<AppSettings>("set_api_key", { key }),
  clearApiKey: () => call<AppSettings>("clear_api_key"),
  backupDatabase: (destination: string) => call<string>("backup_database", { destination }),
};

export type Api = typeof api;
