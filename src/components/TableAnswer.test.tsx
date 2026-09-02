import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { TableAnswer, __testing } from "./TableAnswer";
import type { TemplateColumn } from "@/types";

const columns: TemplateColumn[] = [
  { key: "host", label: "Host" },
  { key: "port", label: "Port" },
  { key: "service", label: "Service" },
];

describe("table answer serialisation", () => {
  it("serialises an empty table to the empty string, not to []", () => {
    // The readiness gate treats a blank answer as unanswered. "[]" would read as filled
    // in, and a required-but-empty table would pass finalization.
    expect(__testing.serializeRows([])).toBe("");
    expect(__testing.serializeRows([{ host: "", port: "  " }])).toBe("");
  });

  it("drops blank rows and trims cells", () => {
    const out = __testing.serializeRows([
      { host: " 10.0.0.5 ", port: "22" },
      { host: "", port: "" },
      { host: "10.0.0.6", port: "80" },
    ]);
    expect(JSON.parse(out)).toEqual([
      { host: "10.0.0.5", port: "22" },
      { host: "10.0.0.6", port: "80" },
    ]);
  });

  it("reads back what it wrote, keyed by column", () => {
    const rows = [{ host: "10.0.0.5", port: "22", service: "ssh" }];
    expect(__testing.parseRows(__testing.serializeRows(rows))).toEqual(rows);
  });

  it("treats malformed stored values as an empty table rather than throwing", () => {
    expect(__testing.parseRows("not json")).toEqual([]);
    expect(__testing.parseRows('{"not":"an array"}')).toEqual([]);
    expect(__testing.parseRows("[1,2,3]")).toEqual([]);
  });

  it("coerces non-string cells instead of dropping them", () => {
    expect(__testing.parseRows('[{"port":443}]')).toEqual([{ port: "443" }]);
  });
});

describe("table answer editor", () => {
  it("adds a row when the add control is clicked", () => {
    const onChange = vi.fn();
    render(
      <TableAnswer
        columns={columns}
        value=""
        label="Exposed services"
        rowLabel="Add service"
        onChange={onChange}
      />
    );
    expect(screen.getByText(/Nothing recorded yet/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /Add service/ }));
    expect(onChange).toHaveBeenCalledTimes(1);
    expect(JSON.parse(onChange.mock.calls[0]![0] as string)).toHaveLength(1);
  });

  it("writes a typed cell back under its column key", () => {
    const onChange = vi.fn();
    render(
      <TableAnswer
        columns={columns}
        value='[{"host":"","port":"","service":""}]'
        label="Exposed services"
        onChange={onChange}
      />
    );
    fireEvent.change(screen.getByLabelText("Host, row 1"), { target: { value: "10.10.10.3" } });
    expect(JSON.parse(onChange.mock.calls[0]![0] as string)).toEqual([{ host: "10.10.10.3" }]);
  });

  it("removes a row", () => {
    const onChange = vi.fn();
    render(
      <TableAnswer
        columns={columns}
        value='[{"host":"a"},{"host":"b"}]'
        label="Exposed services"
        onChange={onChange}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Remove row 1" }));
    expect(JSON.parse(onChange.mock.calls[0]![0] as string)).toEqual([{ host: "b" }]);
  });

  it("reorders rows without losing cells", () => {
    const onChange = vi.fn();
    render(
      <TableAnswer
        columns={columns}
        value='[{"host":"a","port":"1"},{"host":"b","port":"2"}]'
        label="Exposed services"
        onChange={onChange}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Move row 2 up" }));
    expect(JSON.parse(onChange.mock.calls[0]![0] as string)).toEqual([
      { host: "b", port: "2" },
      { host: "a", port: "1" },
    ]);
  });

  it("hides the editing controls when the report is not editable", () => {
    render(
      <TableAnswer
        columns={columns}
        value='[{"host":"a"}]'
        disabled
        label="Exposed services"
        onChange={vi.fn()}
      />
    );
    expect(screen.queryByRole("button", { name: /Add row/ })).toBeNull();
    expect(screen.queryByRole("button", { name: /Remove row/ })).toBeNull();
    expect((screen.getByLabelText("Host, row 1") as HTMLInputElement).disabled).toBe(true);
  });
});
