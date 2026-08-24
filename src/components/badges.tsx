/**
 * Status vocabulary, rendered consistently everywhere.
 *
 * Severity, classification and provenance all appear in several screens; centralising the
 * mapping from value to label and colour means the same finding cannot look Medium in one
 * list and Moderate in another.
 */

import type {
  AiConfidence,
  Classification,
  EngagementStatus,
  PrivacyMode,
  Provenance,
  ReportStatus,
  Severity,
  VerificationStatus,
} from "@/types";

export const SEVERITY_LABELS: Record<Severity, string> = {
  critical: "Critical",
  high: "High",
  medium: "Medium",
  low: "Low",
  informational: "Informational",
};

export const SEVERITY_ORDER: Severity[] = ["critical", "high", "medium", "low", "informational"];

export const CLASSIFICATION_LABELS: Record<Classification, string> = {
  public: "Public",
  internal: "Internal",
  confidential: "Confidential",
  restricted: "Restricted",
};

export const REPORT_STATUS_LABELS: Record<ReportStatus, string> = {
  draft: "Draft",
  review: "In review",
  final: "Final",
  archived: "Archived",
};

export const ENGAGEMENT_STATUS_LABELS: Record<EngagementStatus, string> = {
  draft: "Draft",
  active: "Active",
  review: "Review",
  completed: "Completed",
  archived: "Archived",
};

export const VERIFICATION_LABELS: Record<VerificationStatus, string> = {
  unverified: "Unverified",
  verified: "Verified",
  remediated: "Remediated",
  risk_accepted: "Risk accepted",
  false_positive: "False positive",
};

export const PROVENANCE_LABELS: Record<Provenance, string> = {
  user_provided: "Author",
  ai_wording: "AI wording",
  ai_inference: "AI inference",
  missing_information: "Information required",
};

export const PROVENANCE_HINTS: Record<Provenance, string> = {
  user_provided: "Written by you. This is the only kind that asserts fact on your behalf.",
  ai_wording: "An assistant reworded your material. The facts remain yours.",
  ai_inference:
    "Assistant-drafted prose you have not confirmed. This blocks finalization until you accept it as your own or rewrite it.",
  missing_information: "Marked as a gap. It prints as “Information required.”",
};

export function SeverityBadge({ severity }: { severity: Severity }) {
  return <span className={`badge badge--${severity}`}>{SEVERITY_LABELS[severity]}</span>;
}

export function ClassificationBadge({ value }: { value: Classification }) {
  return <span className={`classification classification--${value}`}>{CLASSIFICATION_LABELS[value]}</span>;
}

export function ReportStatusBadge({ status }: { status: ReportStatus }) {
  const tone =
    status === "final" ? "success" : status === "review" ? "warning" : status === "archived" ? "neutral" : "accent";
  return <span className={`badge badge--${tone}`}>{REPORT_STATUS_LABELS[status]}</span>;
}

export function EngagementStatusBadge({ status }: { status: EngagementStatus }) {
  const tone =
    status === "active" ? "accent" : status === "completed" ? "success" : status === "review" ? "warning" : "neutral";
  return <span className={`badge badge--${tone}`}>{ENGAGEMENT_STATUS_LABELS[status]}</span>;
}

export function VerificationBadge({ status }: { status: VerificationStatus }) {
  const tone =
    status === "verified" || status === "remediated"
      ? "success"
      : status === "false_positive"
        ? "neutral"
        : status === "risk_accepted"
          ? "warning"
          : "outline";
  return <span className={`badge badge--${tone}`}>{VERIFICATION_LABELS[status]}</span>;
}

export function ProvenanceBadge({ provenance }: { provenance: Provenance }) {
  if (provenance === "user_provided") return null;
  const tone =
    provenance === "ai_wording" ? "accent" : provenance === "ai_inference" ? "warning" : "danger";
  return (
    <span className={`badge badge--${tone}`} title={PROVENANCE_HINTS[provenance]}>
      {PROVENANCE_LABELS[provenance]}
    </span>
  );
}

export function ConfidenceBadge({ confidence }: { confidence: AiConfidence }) {
  const tone = confidence === "high" ? "success" : confidence === "medium" ? "warning" : "danger";
  return (
    <span className={`badge badge--${tone}`}>
      AI confidence: {confidence[0]?.toUpperCase()}
      {confidence.slice(1)}
    </span>
  );
}

/**
 * The privacy indicator. Always visible wherever an AI action can be started, because
 * "where does my client's text go?" should never require a trip to Settings.
 */
export function PrivacyBadge({ mode, title }: { mode: PrivacyMode; title?: string }) {
  const label =
    mode === "cloud" ? "OpenAI Cloud" : mode === "local" ? "Local / Private" : "AI disabled";
  return (
    <span className={`privacy privacy--${mode}`} title={title}>
      <span className="privacy__dot" aria-hidden="true" />
      {label}
    </span>
  );
}

export function SeverityConfirmationBadge({ confirmed }: { confirmed: boolean }) {
  return confirmed ? (
    <span className="badge badge--success">Severity confirmed</span>
  ) : (
    <span className="badge badge--warning" title="A person must confirm severity before this finding can appear in a finalized report.">
      Severity unconfirmed
    </span>
  );
}
