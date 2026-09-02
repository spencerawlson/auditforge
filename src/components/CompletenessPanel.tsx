/**
 * The completeness indicator: how much of the required structure a report or postmortem
 * carries, and whether it meets the professional length target for its kind.
 *
 * Content completeness is the headline number. The page target is shown as guidance and is
 * deliberately never a blocker - a concise document that is genuinely complete is flagged
 * for review, not rejected.
 */

import type { Completeness } from "@/types";

const COMPLETENESS_LABELS: Record<Completeness["status"], string> = {
  complete: "Complete",
  incomplete: "Incomplete",
  requires_review: "Requires review",
};

const COMPLETENESS_TONE: Record<Completeness["status"], string> = {
  complete: "success",
  incomplete: "danger",
  requires_review: "warning",
};

export function CompletenessPanel({ completeness }: { completeness: Completeness }) {
  const c = completeness;
  const pct = Math.max(0, Math.min(100, Math.round(c.percent)));
  return (
    <div className="completeness">
      <div className="completeness__head">
        <span className={`badge badge--${COMPLETENESS_TONE[c.status]}`}>
          {COMPLETENESS_LABELS[c.status]}
        </span>
        <span className="completeness__pct">{pct}% complete</span>
      </div>
      <div
        className="completeness__meter"
        role="progressbar"
        aria-valuenow={pct}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-label="Required-section completeness"
      >
        <div
          className={`completeness__bar completeness__bar--${c.status}`}
          style={{ width: `${pct}%` }}
        />
      </div>
      <div className="completeness__meta tiny muted">
        <span>{c.standard_label}</span>
        <span
          title={
            c.meets_page_target
              ? "Meets the minimum length for this type."
              : "Shorter than the target for this type. This is guidance, not a blocker - a concise document that is genuinely complete can still be finalized."
          }
        >
          ~{c.estimated_pages} of {c.min_pages} page min{c.meets_page_target ? " ✓" : ""}
        </span>
      </div>
      {c.missing_sections.length > 0 && (
        <details>
          <summary className="tiny muted" style={{ cursor: "pointer" }}>
            {c.missing_sections.length} required section
            {c.missing_sections.length === 1 ? "" : "s"} still empty
          </summary>
          <ul className="tiny muted" style={{ margin: "6px 0 0", paddingLeft: 18 }}>
            {c.missing_sections.map((label) => (
              <li key={label}>{label}</li>
            ))}
          </ul>
        </details>
      )}
      {c.status === "requires_review" && c.missing_sections.length === 0 && (
        <p className="tiny muted" style={{ margin: 0 }}>
          Every required section has content, but this is shorter than the {c.min_pages}-page
          target. Add substantive detail where it is thin, or finalize it as-is if it is
          genuinely complete.
        </p>
      )}
    </div>
  );
}
