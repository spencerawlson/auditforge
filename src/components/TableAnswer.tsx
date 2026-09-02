import { useMemo } from "react";
import type { TemplateColumn } from "@/types";
import { Button } from "@/components/ui";

/**
 * The editor for a table question — assets, ports, tooling.
 *
 * The rows live in the same answer string as every other question, encoded as a JSON
 * array of objects keyed by column key. Keeping the storage identical to a text answer is
 * what lets tables inherit versioning, the content hash, immutability and diffing without
 * any of those systems learning a new shape.
 *
 * Two rules this component has to honour, both enforced again on the Rust side so a bug
 * here cannot corrupt a stored report:
 *
 *  - A table with no content serialises to the EMPTY STRING, never to "[]". The readiness
 *    gate treats a blank answer as unanswered, and "[]" would read as filled in.
 *  - Rows are keyed by column key, never by position, so reordering columns in a template
 *    cannot shift stored values sideways.
 */

export type TableRow = Record<string, string>;

function parseRows(value: string): TableRow[] {
  const trimmed = value.trim();
  if (!trimmed) return [];
  try {
    const parsed: unknown = JSON.parse(trimmed);
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter((row): row is Record<string, unknown> => typeof row === "object" && row !== null && !Array.isArray(row))
      .map((row) => {
        const out: TableRow = {};
        for (const [key, cell] of Object.entries(row)) {
          out[key] = cell === null || cell === undefined ? "" : String(cell);
        }
        return out;
      });
  } catch {
    // A malformed value shows as an empty table the author can refill, rather than
    // throwing and taking the whole editor down.
    return [];
  }
}

function serializeRows(rows: TableRow[]): string {
  const kept = rows
    .map((row) => {
      const trimmed: TableRow = {};
      for (const [key, cell] of Object.entries(row)) {
        if (cell.trim()) trimmed[key] = cell.trim();
      }
      return trimmed;
    })
    .filter((row) => Object.keys(row).length > 0);
  return kept.length === 0 ? "" : JSON.stringify(kept);
}

interface Props {
  columns: TemplateColumn[];
  value: string;
  disabled?: boolean;
  rowLabel?: string;
  label: string;
  onChange: (value: string) => void;
}

export function TableAnswer({ columns, value, disabled, rowLabel, label, onChange }: Props) {
  const rows = useMemo(() => parseRows(value), [value]);
  const addLabel = rowLabel?.trim() || "Add row";

  function commit(next: TableRow[]) {
    onChange(serializeRows(next));
  }

  function setCell(rowIndex: number, key: string, cell: string) {
    const next = rows.map((row, i) => (i === rowIndex ? { ...row, [key]: cell } : row));
    commit(next);
  }

  function addRow() {
    // A visibly empty row is added so the author has somewhere to type. It is dropped on
    // serialize if they leave it blank, so an abandoned row never reaches the report.
    const blank: TableRow = {};
    for (const column of columns) blank[column.key] = "";
    onChange(JSON.stringify([...rows.map((r) => ({ ...r })), blank]));
  }

  function removeRow(rowIndex: number) {
    commit(rows.filter((_, i) => i !== rowIndex));
  }

  function moveRow(rowIndex: number, delta: number) {
    const target = rowIndex + delta;
    if (target < 0 || target >= rows.length) return;
    const next = [...rows];
    // Read both before writing either. `noUncheckedIndexedAccess` types an index read as
    // possibly-undefined, and the bounds check above is what actually guarantees it is not.
    const a = next[rowIndex] as TableRow;
    const b = next[target] as TableRow;
    next[rowIndex] = b;
    next[target] = a;
    commit(next);
  }

  if (columns.length === 0) {
    return <p className="tiny faint">This table has no columns defined in its template.</p>;
  }

  return (
    <div className="tableAnswer">
      <div className="tableAnswer__scroll">
        <table className="tableAnswer__grid">
          <caption className="sr-only">{label}</caption>
          <thead>
            <tr>
              <th scope="col" className="tableAnswer__num" aria-label="Row" />
              {columns.map((column) => (
                <th key={column.key} scope="col" title={column.help || undefined}>
                  {column.label}
                </th>
              ))}
              {!disabled && <th scope="col" className="tableAnswer__actions" aria-label="Actions" />}
            </tr>
          </thead>
          <tbody>
            {rows.map((row, rowIndex) => (
              <tr key={rowIndex}>
                <td className="tableAnswer__num">{rowIndex + 1}</td>
                {columns.map((column) => (
                  <td key={column.key}>
                    <input
                      className="tableAnswer__cell"
                      value={row[column.key] ?? ""}
                      disabled={disabled}
                      aria-label={`${column.label}, row ${rowIndex + 1}`}
                      onChange={(event) => setCell(rowIndex, column.key, event.target.value)}
                    />
                  </td>
                ))}
                {!disabled && (
                  <td className="tableAnswer__actions">
                    <button
                      type="button"
                      className="tableAnswer__icon"
                      title="Move up"
                      aria-label={`Move row ${rowIndex + 1} up`}
                      disabled={rowIndex === 0}
                      onClick={() => moveRow(rowIndex, -1)}
                    >
                      ↑
                    </button>
                    <button
                      type="button"
                      className="tableAnswer__icon"
                      title="Move down"
                      aria-label={`Move row ${rowIndex + 1} down`}
                      disabled={rowIndex === rows.length - 1}
                      onClick={() => moveRow(rowIndex, 1)}
                    >
                      ↓
                    </button>
                    <button
                      type="button"
                      className="tableAnswer__icon tableAnswer__icon--danger"
                      title="Remove this row"
                      aria-label={`Remove row ${rowIndex + 1}`}
                      onClick={() => removeRow(rowIndex)}
                    >
                      ✕
                    </button>
                  </td>
                )}
              </tr>
            ))}
            {rows.length === 0 && (
              <tr>
                <td className="tableAnswer__empty" colSpan={columns.length + (disabled ? 1 : 2)}>
                  Nothing recorded yet. This table is left out of the report until it has a row.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      {!disabled && (
        <div className="tableAnswer__foot">
          <Button size="sm" variant="ghost" onClick={addRow}>
            + {addLabel}
          </Button>
          <span className="tiny faint">
            {rows.length === 0
              ? "no rows"
              : `${rows.length} row${rows.length === 1 ? "" : "s"}`}
          </span>
        </div>
      )}
    </div>
  );
}

export const __testing = { parseRows, serializeRows };
