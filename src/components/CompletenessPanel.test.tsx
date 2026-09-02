import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { CompletenessPanel } from "./CompletenessPanel";
import type { Completeness } from "@/types";

function make(overrides: Partial<Completeness>): Completeness {
  return {
    status: "complete",
    percent: 100,
    min_pages: 5,
    estimated_pages: 6,
    meets_page_target: true,
    required_sections: [],
    missing_sections: [],
    standard_label: "Engagement report",
    ...overrides,
  };
}

describe("the completeness indicator", () => {
  it("shows the content percentage and the length target", () => {
    render(<CompletenessPanel completeness={make({ percent: 92, estimated_pages: 6 })} />);
    expect(screen.getByText("92% complete")).toBeInTheDocument();
    expect(screen.getByText(/6 of 5 page min/)).toBeInTheDocument();
    // The progress bar reports the same percentage for assistive tech.
    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "92");
  });

  it("lists the required sections that are still empty when incomplete", () => {
    render(
      <CompletenessPanel
        completeness={make({
          status: "incomplete",
          percent: 60,
          missing_sections: ["Conclusion", "Recommendations"],
        })}
      />,
    );
    expect(screen.getByText("Incomplete")).toBeInTheDocument();
    expect(screen.getByText(/2 required sections still empty/)).toBeInTheDocument();
    expect(screen.getByText("Conclusion")).toBeInTheDocument();
    expect(screen.getByText("Recommendations")).toBeInTheDocument();
  });

  it("flags a complete-but-short document for review rather than rejecting it", () => {
    render(
      <CompletenessPanel
        completeness={make({
          status: "requires_review",
          percent: 100,
          estimated_pages: 3,
          meets_page_target: false,
        })}
      />,
    );
    expect(screen.getByText("Requires review")).toBeInTheDocument();
    // 100% of the required content is present even though it is short.
    expect(screen.getByText("100% complete")).toBeInTheDocument();
    expect(screen.getByText(/shorter than the 5-page target/)).toBeInTheDocument();
    // Nothing is listed as missing - the shortfall is length, not a gap.
    expect(screen.queryByText(/required sections? still empty/)).not.toBeInTheDocument();
  });
});
