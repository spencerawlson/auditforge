import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { StartupStatus } from "@/types";

const startupStatus = vi.fn();
const register = vi.fn();
const login = vi.fn();

vi.mock("@/services/api", () => ({
  api: {
    startupStatus: () => startupStatus(),
    register: (input: unknown) => register(input),
    login: (username: string, password: string) => login(username, password),
    getSettings: () => Promise.resolve(null),
    sessionRemaining: () => Promise.resolve(null),
    currentUser: () => Promise.resolve(null),
    passwordStrength: () => Promise.resolve(3),
  },
  onSignedOut: () => () => undefined,
  BackendError: class extends Error {
    kind = "internal";
  },
}));

// Imported after the mock so the module graph picks it up.
const { AuthScreen } = await import("./AuthScreen");
const { SessionProvider } = await import("./SessionProvider");

function status(overrides: Partial<StartupStatus> = {}): StartupStatus {
  return {
    has_account: true,
    signed_in: false,
    user: null,
    version: "0.1.0",
    data_dir: "/tmp/auditforge",
    secret_backend: "OS keychain",
    secrets_os_backed: true,
    ...overrides,
  };
}

function renderScreen() {
  return render(
    <SessionProvider>
      <AuthScreen />
    </SessionProvider>,
  );
}

describe("the sign-in screen", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("always offers both signing in and creating an account", async () => {
    startupStatus.mockResolvedValue(status());
    renderScreen();

    await waitFor(() => expect(screen.getByRole("tab", { name: "Sign in" })).toBeInTheDocument());
    expect(screen.getByRole("tab", { name: "Create account" })).toBeInTheDocument();
  });

  it("starts on sign in when the machine already has an account", async () => {
    startupStatus.mockResolvedValue(status({ has_account: true }));
    renderScreen();

    await waitFor(() =>
      expect(screen.getByRole("tab", { name: "Sign in" })).toHaveAttribute(
        "aria-selected",
        "true",
      ),
    );
    expect(screen.getByRole("button", { name: "Sign in" })).toBeInTheDocument();
  });

  it("starts on create account on a fresh install, and still offers sign in", async () => {
    startupStatus.mockResolvedValue(status({ has_account: false }));
    renderScreen();

    await waitFor(() =>
      expect(screen.getByRole("tab", { name: "Create account" })).toHaveAttribute(
        "aria-selected",
        "true",
      ),
    );
    expect(screen.getByRole("tab", { name: "Sign in" })).toBeEnabled();
  });

  it("switches between the two in both directions", async () => {
    const user = userEvent.setup();
    startupStatus.mockResolvedValue(status({ has_account: true }));
    renderScreen();

    await waitFor(() => expect(screen.getByLabelText("Username")).toBeInTheDocument());

    await user.click(screen.getByRole("tab", { name: "Create account" }));
    expect(screen.getByLabelText("Full name")).toBeInTheDocument();

    await user.click(screen.getByRole("tab", { name: "Sign in" }));
    expect(screen.getByLabelText("Password")).toBeInTheDocument();
  });

  it("tells a first-time user where to go when no account exists yet", async () => {
    const user = userEvent.setup();
    startupStatus.mockResolvedValue(status({ has_account: false }));
    renderScreen();

    await waitFor(() => expect(screen.getByLabelText("Full name")).toBeInTheDocument());
    await user.click(screen.getByRole("tab", { name: "Sign in" }));

    expect(screen.getByText("No account on this machine yet")).toBeInTheDocument();
  });
});
