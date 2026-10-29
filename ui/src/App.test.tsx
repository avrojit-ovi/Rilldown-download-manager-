import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";

import App from "@/App";
import { useUiStore } from "@/stores/ui";

beforeEach(() => {
  window.localStorage.clear();
  useUiStore.setState({
    selection: { kind: "library", filter: "all" },
    search: "",
    theme: "system",
    language: "en",
  });
});

describe("application shell", () => {
  it("renders the empty library and opens the add dialog", async () => {
    const user = userEvent.setup();
    render(<App />);
    expect(screen.getByRole("heading", { level: 1, name: "All downloads" })).toBeInTheDocument();
    expect(screen.getByText("Nothing in the stream yet")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Add URL" }));
    expect(screen.getByRole("dialog", { name: "Add download" })).toBeInTheDocument();
  });

  it("switches between library filters", async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.click(screen.getByRole("button", { name: "Completed" }));

    expect(screen.getByRole("heading", { level: 1, name: "Completed" })).toBeInTheDocument();
    expect(screen.getByText("No completed downloads yet")).toBeInTheDocument();
  });

  it("shows a search empty state and clears it", async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.type(screen.getByRole("searchbox"), "invoice");

    expect(screen.getByText("No matching downloads")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Clear search" }));
    expect(screen.getByText("Nothing in the stream yet")).toBeInTheDocument();
  });

  it("switches the interface to Bangla", async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.click(screen.getByRole("button", { name: /Language/ }));

    expect(screen.getByRole("heading", { level: 1, name: "সব ডাউনলোড" })).toBeInTheDocument();
  });
});
