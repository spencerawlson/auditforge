import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import {
  ClassificationBadge,
  ConfidenceBadge,
  PrivacyBadge,
  ProvenanceBadge,
  SeverityBadge,
  SeverityConfirmationBadge,
  SEVERITY_ORDER,
} from "./badges";

describe("status vocabulary", () => {
  it("renders every severity with a matching class", () => {
    for (const severity of SEVERITY_ORDER) {
      const { container, unmount } = render(<SeverityBadge severity={severity} />);
      expect(container.firstElementChild).toHaveClass(`badge--${severity}`);
      unmount();
    }
  });

  it("shows the classification banner wording used in the document", () => {
    render(<ClassificationBadge value="restricted" />);
    expect(screen.getByText("Restricted")).toBeInTheDocument();
  });

  it("says nothing when text is the author's own", () => {
    const { container } = render(<ProvenanceBadge provenance="user_provided" />);
    expect(container).toBeEmptyDOMElement();
  });

  it("marks AI inference distinctly from AI wording", () => {
    const { container: inference } = render(<ProvenanceBadge provenance="ai_inference" />);
    const { container: wording } = render(<ProvenanceBadge provenance="ai_wording" />);
    expect(inference.firstElementChild).toHaveClass("badge--warning");
    expect(wording.firstElementChild).toHaveClass("badge--accent");
  });

  it("flags an unconfirmed severity and explains why it matters", () => {
    render(<SeverityConfirmationBadge confirmed={false} />);
    const badge = screen.getByText("Severity unconfirmed");
    expect(badge).toBeInTheDocument();
    expect(badge).toHaveAttribute("title", expect.stringContaining("finalized report"));
  });

  it("names the destination in the privacy indicator", () => {
    const { rerender } = render(<PrivacyBadge mode="cloud" />);
    expect(screen.getByText("OpenAI Cloud")).toBeInTheDocument();

    rerender(<PrivacyBadge mode="local" />);
    expect(screen.getByText("Local / Private")).toBeInTheDocument();

    rerender(<PrivacyBadge mode="offline" />);
    expect(screen.getByText("AI disabled")).toBeInTheDocument();
  });

  it("labels AI confidence for the reviewer", () => {
    render(<ConfidenceBadge confidence="low" />);
    expect(screen.getByText(/AI confidence: Low/)).toBeInTheDocument();
  });
});
