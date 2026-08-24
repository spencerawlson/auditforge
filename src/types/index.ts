/**
 * TypeScript mirrors of the Rust domain types.
 *
 * These are hand-written rather than generated so the frontend contract is reviewable in
 * one place. Every string-union here matches a `string_enum!` in `models/common.rs`; the
 * Rust side serializes and deserializes the same spellings, so a mismatch fails loudly at
 * the boundary rather than silently defaulting.
 */

export type Severity = "critical" | "high" | "medium" | "low" | "informational";
export type Classification = "public" | "internal" | "confidential" | "restricted";
export type EngagementStatus = "draft" | "active" | "review" | "completed" | "archived";
export type ReportStatus = "draft" | "review" | "final" | "archived";
export type VerificationStatus =
  | "unverified"
  | "verified"
  | "remediated"
  | "risk_accepted"
  | "false_positive";
export type EvidenceKind =
  | "image"
  | "log"
  | "text"
  | "csv"
  | "json"
  | "capture"
  | "archive"
  | "other";
export type IncidentStatus = "open" | "investigating" | "contained" | "resolved" | "closed";
export type PostmortemStatus = "draft" | "review" | "complete";
export type CorrectiveActionStatus =
  | "open"
  | "in_progress"
  | "complete"
  | "verified"
  | "cancelled";
export type EntityType =
  | "engagement"
  | "report"
  | "finding"
  | "incident"
  | "postmortem"
  | "evidence";

/** Where a piece of text came from. The heart of the AI safety model. */
export type Provenance =
  | "user_provided"
  | "ai_wording"
  | "ai_inference"
  | "missing_information";

export type ReportType =
  | "penetration_test"
  | "vulnerability_assessment"
  | "security_assessment"
  | "network_assessment"
  | "incident_report"
  | "postmortem"
  | "cloud_security_assessment"
  | "compliance_audit"
  | "engagement_report"
  | "executive_summary"
  | "custom";

export type AiProviderKind = "openai" | "local" | "disabled";
export type PrivacyMode = "offline" | "local" | "cloud";
export type AiConfidence = "high" | "medium" | "low";
export type AiDecision = "accepted" | "edited" | "rejected";
export type SummaryKind = "executive" | "technical" | "incident" | "finding";
export type AiFunction =
  | "improve_writing"
  | "rewrite"
  | "technical_rewrite"
  | "executive_rewrite"
  | "summarize"
  | "expand"
  | "report_review"
  | "finding_analysis"
  | "executive_summary";
export type ReviewCategory =
  | "grammar"
  | "clarity"
  | "contradiction"
  | "missing_information"
  | "unsupported_claim"
  | "inconsistent_terminology"
  | "inconsistent_severity"
  | "incomplete_remediation"
  | "missing_evidence"
  | "timeline_inconsistency"
  | "other";

export type ExportFormat = "pdf" | "html" | "json" | "markdown" | "csv";

/** RFC 3339 UTC instant, e.g. `2026-08-24T09:15:00Z`. */
export type Timestamp = string;

// ---------------------------------------------------------------- errors

export interface ApiError {
  kind:
    | "validation"
    | "not_found"
    | "unauthorized"
    | "forbidden"
    | "locked"
    | "immutable"
    | "ai_policy"
    | "ai_provider"
    | "database"
    | "io"
    | "serialization"
    | "internal";
  message: string;
  retry_after_secs?: number;
}

// ---------------------------------------------------------------- account

export interface User {
  id: string;
  username: string;
  full_name: string;
  job_title: string;
  email: string;
  organization_id: string | null;
  organization_name: string;
  is_active: boolean;
  last_login_at: Timestamp | null;
  password_changed_at: Timestamp;
  created_at: Timestamp;
  updated_at: Timestamp;
}

export interface StartupStatus {
  has_account: boolean;
  signed_in: boolean;
  user: User | null;
  version: string;
  data_dir: string;
  secret_backend: string;
  secrets_os_backed: boolean;
}

export interface SignedIn {
  user: User;
  expires_at: Timestamp;
}

export interface NewUser {
  username: string;
  password: string;
  confirm_password: string;
  full_name: string;
  organization: string;
  job_title: string;
  email?: string;
}

export interface ProfileUpdate {
  full_name?: string;
  organization?: string;
  job_title?: string;
  email?: string;
}

export interface AuthorSnapshot {
  user_id: string;
  username: string;
  full_name: string;
  organization: string;
  job_title: string;
  captured_at: Timestamp;
}

// ---------------------------------------------------------------- engagements

export interface TeamMember {
  id: string;
  name: string;
  role: string;
  email: string;
  sort_order: number;
}

export interface NewTeamMember {
  name: string;
  role?: string;
  email?: string;
}

export interface Engagement {
  id: string;
  engagement_ref: string;
  name: string;
  client_name: string;
  organization_id: string | null;
  owner_user_id: string;
  status: EngagementStatus;
  start_date: string | null;
  end_date: string | null;
  objectives: string;
  scope: string;
  out_of_scope: string;
  authorization_ref: string;
  rules_of_engagement: string;
  notes: string;
  team: TeamMember[];
  created_at: Timestamp;
  updated_at: Timestamp;
}

/** `EngagementSummary` flattens the engagement, so it has every field plus the counts. */
export interface EngagementSummary extends Engagement {
  report_count: number;
  finding_count: number;
  evidence_count: number;
}

export interface NewEngagement {
  name: string;
  client_name?: string;
  start_date?: string | null;
  end_date?: string | null;
  objectives?: string;
  scope?: string;
  out_of_scope?: string;
  authorization_ref?: string;
  rules_of_engagement?: string;
  notes?: string;
  team?: NewTeamMember[];
}

export interface EngagementUpdate {
  name?: string;
  client_name?: string;
  status?: EngagementStatus;
  /** Double option: omitted leaves it alone, `null` clears it. */
  start_date?: string | null;
  end_date?: string | null;
  objectives?: string;
  scope?: string;
  out_of_scope?: string;
  authorization_ref?: string;
  rules_of_engagement?: string;
  notes?: string;
  team?: NewTeamMember[];
}

export interface EngagementFilter {
  status?: EngagementStatus;
  search?: string;
  limit?: number;
}

// ---------------------------------------------------------------- findings

export interface Finding {
  id: string;
  finding_ref: string;
  engagement_id: string | null;
  title: string;
  severity: Severity;
  severity_confirmed: boolean;
  category: string;
  affected_asset: string;
  description: string;
  technical_details: string;
  impact: string;
  likelihood: string;
  risk: string;
  root_cause: string;
  remediation: string;
  recommendation: string;
  verification_status: VerificationStatus;
  reference_links: string;
  cvss_vector: string;
  cvss_score: number | null;
  provenance: Record<string, Provenance>;
  author_user_id: string;
  author_name: string;
  evidence_ids: string[];
  created_at: Timestamp;
  updated_at: Timestamp;
}

export interface NewFinding {
  title: string;
  engagement_id?: string | null;
  severity?: Severity;
  category?: string;
  affected_asset?: string;
  description?: string;
  technical_details?: string;
  impact?: string;
  likelihood?: string;
  risk?: string;
  root_cause?: string;
  remediation?: string;
  recommendation?: string;
  reference_links?: string;
  cvss_vector?: string;
  cvss_score?: number | null;
}

export interface FindingUpdate {
  title?: string;
  engagement_id?: string | null;
  category?: string;
  affected_asset?: string;
  description?: string;
  technical_details?: string;
  impact?: string;
  likelihood?: string;
  risk?: string;
  root_cause?: string;
  remediation?: string;
  recommendation?: string;
  verification_status?: VerificationStatus;
  reference_links?: string;
  cvss_vector?: string;
  cvss_score?: number | null;
  provenance?: Record<string, Provenance>;
}

/** Severity travels on its own so confirmation is always deliberate. */
export interface SeverityDecision {
  severity: Severity;
  confirmed_by_user: boolean;
  rationale?: string;
}

export interface FindingFilter {
  engagement_id?: string;
  severity?: Severity;
  verification_status?: VerificationStatus;
  search?: string;
  limit?: number;
}

// ---------------------------------------------------------------- evidence

export interface EvidenceLink {
  id: string;
  target_type: EntityType;
  target_id: string;
  created_at: Timestamp;
}

export interface Evidence {
  id: string;
  evidence_ref: string;
  engagement_id: string | null;
  title: string;
  description: string;
  original_filename: string;
  stored_filename: string;
  relative_path: string;
  mime_type: string;
  kind: EvidenceKind;
  byte_size: number;
  sha256: string;
  is_encrypted: boolean;
  captured_at: Timestamp | null;
  created_by: string;
  created_at: Timestamp;
  updated_at: Timestamp;
  links: EvidenceLink[];
}

export interface EvidenceLinkRequest {
  target_type: EntityType;
  target_id: string;
}

export interface NewEvidence {
  source_path: string;
  title: string;
  description?: string;
  engagement_id?: string | null;
  captured_at?: Timestamp | null;
  encrypt?: boolean;
  acknowledge_executable?: boolean;
  link_target?: EvidenceLinkRequest | null;
}

export interface EvidenceUpdate {
  title?: string;
  description?: string;
  engagement_id?: string | null;
  captured_at?: Timestamp | null;
}

export interface EvidenceFilter {
  engagement_id?: string;
  kind?: EvidenceKind;
  target_type?: EntityType;
  target_id?: string;
  search?: string;
  limit?: number;
}

// ---------------------------------------------------------------- incidents

export interface Incident {
  id: string;
  incident_ref: string;
  engagement_id: string | null;
  title: string;
  detected_at: Timestamp | null;
  resolved_at: Timestamp | null;
  reporter: string;
  severity: Severity;
  severity_confirmed: boolean;
  affected_systems: string;
  description: string;
  impact: string;
  timeline: string;
  detection_method: string;
  response_actions: string;
  root_cause: string;
  remediation: string;
  lessons_learned: string;
  status: IncidentStatus;
  created_by: string;
  created_at: Timestamp;
  updated_at: Timestamp;
}

export interface NewIncident {
  title: string;
  engagement_id?: string | null;
  detected_at?: Timestamp | null;
  reporter?: string;
  affected_systems?: string;
  description?: string;
  impact?: string;
  detection_method?: string;
}

export interface IncidentUpdate {
  title?: string;
  engagement_id?: string | null;
  detected_at?: Timestamp | null;
  resolved_at?: Timestamp | null;
  reporter?: string;
  affected_systems?: string;
  description?: string;
  impact?: string;
  timeline?: string;
  detection_method?: string;
  response_actions?: string;
  root_cause?: string;
  remediation?: string;
  lessons_learned?: string;
  status?: IncidentStatus;
}

export interface IncidentFilter {
  engagement_id?: string;
  status?: IncidentStatus;
  severity?: Severity;
  search?: string;
  limit?: number;
}

// ---------------------------------------------------------------- postmortems

export interface AnswerValue {
  question_key: string;
  value: string;
  provenance: Provenance;
  ai_request_id?: string | null;
  updated_at: Timestamp;
}

export interface CorrectiveAction {
  id: string;
  action_ref: string;
  postmortem_id: string | null;
  incident_id: string | null;
  finding_id: string | null;
  description: string;
  owner: string;
  due_date: string | null;
  status: CorrectiveActionStatus;
  verification_method: string;
  created_at: Timestamp;
  updated_at: Timestamp;
}

export interface NewCorrectiveAction {
  description: string;
  owner?: string;
  due_date?: string | null;
  verification_method?: string;
  postmortem_id?: string | null;
  incident_id?: string | null;
  finding_id?: string | null;
}

export interface CorrectiveActionUpdate {
  description?: string;
  owner?: string;
  due_date?: string | null;
  status?: CorrectiveActionStatus;
  verification_method?: string;
}

export interface Postmortem {
  id: string;
  postmortem_ref: string;
  title: string;
  incident_id: string | null;
  engagement_id: string | null;
  status: PostmortemStatus;
  answers: Record<string, AnswerValue>;
  corrective_actions: CorrectiveAction[];
  created_by: string;
  created_at: Timestamp;
  updated_at: Timestamp;
}

export interface NewPostmortem {
  title: string;
  incident_id?: string | null;
  engagement_id?: string | null;
}

export interface PostmortemAnswerInput {
  question_key: string;
  value: string;
  provenance?: Provenance;
  ai_request_id?: string | null;
}

export interface PostmortemUpdate {
  title?: string;
  status?: PostmortemStatus;
  incident_id?: string | null;
  engagement_id?: string | null;
  answers?: PostmortemAnswerInput[];
}

export interface GuidedQuestion {
  key: string;
  prompt: string;
}

export interface PostmortemCompletion {
  answered: number;
  total: number;
  unanswered: GuidedQuestion[];
}

// ---------------------------------------------------------------- templates

export type SectionKind = "guided" | "findings" | "evidence";
export type InputKind = "text" | "textarea" | "date" | "list";

export interface TemplateQuestion {
  key: string;
  label: string;
  help: string;
  input: InputKind;
  required: boolean;
  placeholder: string;
}

export interface TemplateSection {
  key: string;
  title: string;
  order: number;
  kind: SectionKind;
  required: boolean;
  guidance: string;
  questions: TemplateQuestion[];
}

export interface ReportTemplate {
  key: string;
  name: string;
  report_type: ReportType;
  version: number;
  description: string;
  default_classification: Classification;
  sections: TemplateSection[];
  source: "builtin" | "user";
}

export interface TemplateSummary {
  key: string;
  name: string;
  report_type: ReportType;
  description: string;
  section_count: number;
  question_count: number;
  source: "builtin" | "user";
  default_classification: Classification;
}

// ---------------------------------------------------------------- reports

export interface EngagementSnapshot {
  id: string;
  engagement_ref: string;
  name: string;
  client_name: string;
  start_date: string | null;
  end_date: string | null;
  team: string[];
}

export interface FindingSnapshot {
  finding_ref: string;
  title: string;
  severity: Severity;
  category: string;
  affected_asset: string;
  description: string;
  technical_details: string;
  impact: string;
  likelihood: string;
  risk: string;
  root_cause: string;
  remediation: string;
  recommendation: string;
  verification_status: VerificationStatus;
  reference_links: string;
  cvss_vector: string;
  cvss_score: number | null;
  provenance: Record<string, Provenance>;
  author_name: string;
  evidence_refs: string[];
}

export interface EvidenceSnapshot {
  evidence_ref: string;
  title: string;
  description: string;
  original_filename: string;
  relative_path: string;
  mime_type: string;
  kind: EvidenceKind;
  byte_size: number;
  sha256: string;
  captured_at: Timestamp | null;
}

export interface SectionContent {
  key: string;
  title: string;
  order: number;
  included: boolean;
  answers: AnswerValue[];
}

export interface ReportContent {
  schema_version: number;
  template_key: string;
  report_type: ReportType;
  title: string;
  classification: Classification;
  author: AuthorSnapshot;
  engagement: EngagementSnapshot;
  sections: SectionContent[];
  findings: FindingSnapshot[];
  evidence: EvidenceSnapshot[];
  report_ref: string;
  version_label: string;
  generated_at: Timestamp;
}

export interface Report {
  id: string;
  report_ref: string;
  engagement_id: string | null;
  template_key: string;
  report_type: ReportType;
  title: string;
  classification: Classification;
  status: ReportStatus;
  current_version_id: string | null;
  created_by: string;
  created_at: Timestamp;
  updated_at: Timestamp;
  finalized_at: Timestamp | null;
}

export interface ReportSummary extends Report {
  engagement_name: string | null;
  current_version_label: string | null;
  author_name: string;
  finding_count: number;
}

export interface ReportVersion {
  id: string;
  report_id: string;
  version_major: number;
  version_minor: number;
  version_label: string;
  status: ReportStatus;
  content: ReportContent;
  author: AuthorSnapshot;
  content_hash: string;
  notes: string;
  is_immutable: boolean;
  created_by: string;
  created_at: Timestamp;
  finalized_at: Timestamp | null;
}

export interface ReportVersionSummary {
  id: string;
  report_id: string;
  version_label: string;
  status: ReportStatus;
  is_immutable: boolean;
  notes: string;
  author_name: string;
  content_hash: string;
  created_at: Timestamp;
  finalized_at: Timestamp | null;
}

export interface ValidationIssue {
  severity: "blocker" | "warning";
  code: string;
  message: string;
  section_key?: string;
  question_key?: string;
}

export interface ReportReadiness {
  can_finalize: boolean;
  issues: ValidationIssue[];
}

export interface NewReport {
  title: string;
  template_key: string;
  engagement_id?: string | null;
  classification?: Classification;
}

export interface AnswerInput {
  section_key: string;
  question_key: string;
  value: string;
  provenance?: Provenance;
  ai_request_id?: string | null;
}

export interface ReportDraftUpdate {
  title?: string;
  classification?: Classification;
  answers?: AnswerInput[];
  included_sections?: string[];
  finding_ids?: string[];
  evidence_ids?: string[];
  notes?: string;
}

export interface ReportFilter {
  engagement_id?: string;
  status?: ReportStatus;
  search?: string;
  limit?: number;
}

export interface ReportWorkspace {
  report: Report;
  version: ReportVersion;
  template: ReportTemplate | null;
  readiness: ReportReadiness;
  history: ReportVersionSummary[];
}

// ---------------------------------------------------------------- AI

export interface AiStatus {
  provider: AiProviderKind;
  privacy_mode: PrivacyMode;
  banner: string;
  destination: string;
  model: string;
  enabled: boolean;
  leaves_machine: boolean;
  redaction_enabled: boolean;
}

export interface DiffSegment {
  kind: "equal" | "insert" | "delete";
  text: string;
}

export interface AiSuggestion {
  request_id: string;
  function: AiFunction;
  provider: AiProviderKind;
  privacy_mode: PrivacyMode;
  model: string;
  original: string;
  suggestion: string;
  confidence: AiConfidence;
  provenance: Provenance;
  missing_information: string[];
  notes: string[];
  diff: DiffSegment[];
  created_at: Timestamp;
}

export interface ReviewWarning {
  category: ReviewCategory;
  message: string;
  section_key?: string;
  quote?: string;
}

export interface AiReviewResult {
  request_id: string;
  provider: AiProviderKind;
  privacy_mode: PrivacyMode;
  model: string;
  summary: string;
  confidence: AiConfidence;
  warnings: ReviewWarning[];
  created_at: Timestamp;
}

export interface AiTextRequest {
  function: AiFunction;
  text: string;
  summary_kind?: SummaryKind | null;
  context?: string;
  target_type?: string;
  target_id?: string;
}

export interface SensitiveDetection {
  kind:
    | "private_key_block"
    | "access_key_id"
    | "bearer_token"
    | "credential"
    | "email"
    | "ip_v4"
    | "ip_v6";
  text: string;
}

export interface RedactionPreview {
  detections: SensitiveDetection[];
  redacted_text: string;
}

export interface AiRequestRow {
  id: string;
  function: string;
  provider: string;
  privacy_mode: string;
  model: string;
  status: string;
  confidence: string;
  decision: string | null;
  input_chars: number;
  output_chars: number;
  created_at: string;
  decided_at: string | null;
}

// ---------------------------------------------------------------- settings

export interface AiSettings {
  provider: AiProviderKind;
  openai_model: string;
  openai_base_url: string;
  local_endpoint: string;
  local_model: string;
  temperature: number;
  max_output_tokens: number;
  request_timeout_secs: number;
  cloud_consent: boolean;
  redact_before_send: boolean;
  has_api_key: boolean;
}

export interface SecuritySettings {
  idle_timeout_secs: number;
  absolute_timeout_secs: number;
  encrypt_evidence_by_default: boolean;
  warn_on_risky_evidence: boolean;
}

export interface PdfSettings {
  page_size: "a4" | "letter";
  include_table_of_contents: boolean;
  include_evidence_appendix: boolean;
  include_evidence_images: boolean;
  show_provenance_markers: boolean;
  footer_note: string;
}

export interface ReportSettings {
  default_template_key: string;
  default_classification: Classification;
  pdf: PdfSettings;
}

export interface UiSettings {
  theme: "system" | "light" | "dark";
  compact_density: boolean;
}

export interface AppSettings {
  security: SecuritySettings;
  ai: AiSettings;
  reports: ReportSettings;
  ui: UiSettings;
}

// ---------------------------------------------------------------- search and audit

export type HitKind =
  | "engagement"
  | "report"
  | "finding"
  | "incident"
  | "postmortem"
  | "evidence";

export interface SearchHit {
  kind: HitKind;
  id: string;
  reference: string;
  title: string;
  subtitle: string;
  severity?: Severity;
  status: string;
  updated_at: Timestamp;
  score: number;
}

export interface SearchQuery {
  text?: string;
  kinds?: HitKind[];
  severity?: Severity;
  updated_from?: string;
  updated_to?: string;
  tag?: string;
  limit?: number;
}

export interface AuditEntry {
  id: string;
  occurred_at: Timestamp;
  user_id: string | null;
  username: string;
  action: string;
  target_type: string;
  target_id: string;
  outcome: "success" | "failure" | "denied";
  detail: Record<string, unknown>;
}

export interface AuditQuery {
  action?: string;
  user_id?: string;
  target_type?: string;
  target_id?: string;
  since?: Timestamp;
  until?: Timestamp;
  limit?: number;
}

// ---------------------------------------------------------------- dashboard

export interface Counts {
  engagements: number;
  reports: number;
  findings: number;
  evidence: number;
  incidents: number;
  postmortems: number;
  unconfirmed_severities: number;
}

export interface Dashboard {
  user_id: string;
  counts: Counts;
  recent_reports: ReportSummary[];
  drafts: ReportSummary[];
  awaiting_review: ReportSummary[];
  ready_to_export: ReportSummary[];
  active_engagements: EngagementSummary[];
  recent_findings: Finding[];
  recent_postmortems: Postmortem[];
}
