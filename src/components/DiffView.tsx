/**
 * The three views of an AI suggestion: the original, the suggestion, and the changes.
 *
 * Showing all three is the point. The author is being asked to approve an edit to their
 * own words, and they cannot do that responsibly from the result alone.
 */

import type { DiffSegment } from "@/types";

export function DiffView({ diff }: { diff: DiffSegment[] }) {
  if (diff.length === 0) {
    return <p className="muted small">No changes.</p>;
  }
  return (
    <p className="diff">
      {diff.map((segment, index) => {
        if (segment.kind === "insert") {
          return <ins key={index}>{segment.text}</ins>;
        }
        if (segment.kind === "delete") {
          return <del key={index}>{segment.text}</del>;
        }
        return <span key={index}>{segment.text}</span>;
      })}
    </p>
  );
}

export function CompareView({ original, suggestion }: { original: string; suggestion: string }) {
  return (
    <div className="compare">
      <div className="compare__pane">
        <div className="compare__label">Original</div>
        <div className="compare__text">{original || <span className="faint">(empty)</span>}</div>
      </div>
      <div className="compare__pane">
        <div className="compare__label">AI suggestion</div>
        <div className="compare__text">{suggestion}</div>
      </div>
    </div>
  );
}
