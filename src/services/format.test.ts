import { describe, expect, it } from "vitest";

import { formatBytes, formatDate, formatDateTime, humanize, truncate } from "./format";

describe("display formatting", () => {
  it("renders sizes the way the report appendix does", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(2048)).toBe("2.0 KiB");
    expect(formatBytes(5 * 1024 * 1024)).toBe("5.0 MiB");
  });

  it("shows a dash rather than an empty cell for missing dates", () => {
    expect(formatDate(null)).toBe("—");
    expect(formatDate(undefined)).toBe("—");
    expect(formatDateTime(null)).toBe("—");
  });

  it("passes through a value it cannot parse instead of showing Invalid Date", () => {
    expect(formatDate("not-a-date")).toBe("not-a-date");
  });

  it("formats a real timestamp", () => {
    const formatted = formatDate("2026-08-24T09:15:00Z");
    expect(formatted).toContain("2026");
    expect(formatted).not.toBe("—");
  });

  it("humanizes keys the same way the PDF renderer does", () => {
    expect(humanize("exec_purpose")).toBe("Exec Purpose");
    expect(humanize("false_positive")).toBe("False Positive");
    expect(humanize("")).toBe("");
  });

  it("truncates without cutting mid-word runs of whitespace", () => {
    expect(truncate("short text")).toBe("short text");
    expect(truncate("a  b\n c")).toBe("a b c");
    const long = "x".repeat(200);
    expect(truncate(long, 20)).toHaveLength(20);
    expect(truncate(long, 20).endsWith("…")).toBe(true);
  });
});
