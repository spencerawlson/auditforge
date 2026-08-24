import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { DiffSegment } from "@/types";

import { CompareView, DiffView } from "./DiffView";

const DIFF: DiffSegment[] = [
  { kind: "equal", text: "The host " },
  { kind: "delete", text: "was runing " },
  { kind: "insert", text: "was running " },
  { kind: "equal", text: "an old TLS version." },
];

describe("the AI review views", () => {
  it("marks insertions and deletions so the author can see the edit", () => {
    const { container } = render(<DiffView diff={DIFF} />);
    expect(container.querySelector("ins")).toHaveTextContent("was running");
    expect(container.querySelector("del")).toHaveTextContent("was runing");
    expect(container).toHaveTextContent("The host");
  });

  it("says so plainly when nothing changed", () => {
    render(<DiffView diff={[]} />);
    expect(screen.getByText("No changes.")).toBeInTheDocument();
  });

  it("always shows the original beside the suggestion", () => {
    render(<CompareView original="the box was old" suggestion="The host was outdated." />);
    expect(screen.getByText("Original")).toBeInTheDocument();
    expect(screen.getByText("AI suggestion")).toBeInTheDocument();
    expect(screen.getByText("the box was old")).toBeInTheDocument();
    expect(screen.getByText("The host was outdated.")).toBeInTheDocument();
  });

  it("marks an empty original rather than showing a blank pane", () => {
    render(<CompareView original="" suggestion="Drafted text." />);
    expect(screen.getByText("(empty)")).toBeInTheDocument();
  });
});
