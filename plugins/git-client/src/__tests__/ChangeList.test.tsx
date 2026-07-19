import "@testing-library/jest-dom/vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, describe, expect, it } from "vitest";
import type { ParsedChangeEntry } from "@git-ramus/contracts";
import { ChangeList } from "../components/ChangeList";

afterEach(cleanup);

const changes: ParsedChangeEntry[] = [change("src/alpha.ts"), change("src/beta.ts")];

describe("ChangeList", () => {
  it("selects one repository-relative path without selecting its siblings", async () => {
    const user = userEvent.setup();
    render(<Harness />);

    await user.click(screen.getByRole("checkbox", { name: "Select src/alpha.ts" }));

    expect(screen.getByTestId("selected-paths")).toHaveTextContent("src/alpha.ts");
    expect(screen.getByRole("checkbox", { name: "Select src/alpha.ts" })).toBeChecked();
    expect(screen.getByRole("checkbox", { name: "Select src/beta.ts" })).not.toBeChecked();
    expect(screen.getByRole("checkbox", { name: "Select all Unstaged" })).not.toBeChecked();
  });

  it("selects and clears every visible path through the group checkbox", async () => {
    const user = userEvent.setup();
    render(<Harness />);

    const selectAll = screen.getByRole("checkbox", { name: "Select all Unstaged" });
    await user.click(selectAll);
    expect(screen.getByTestId("selected-paths")).toHaveTextContent("src/alpha.ts,src/beta.ts");
    expect(selectAll).toBeChecked();

    await user.click(selectAll);
    expect(screen.getByTestId("selected-paths")).toBeEmptyDOMElement();
    expect(selectAll).not.toBeChecked();
  });
});

function Harness() {
  const [selectedPaths, setSelectedPaths] = useState<string[]>([]);
  return (
    <>
      <ChangeList
        title="Unstaged"
        changes={changes}
        selectedPaths={selectedPaths}
        onSelectionChange={setSelectedPaths}
      />
      <output data-testid="selected-paths">{selectedPaths.join(",")}</output>
    </>
  );
}

function change(path: string): ParsedChangeEntry {
  return {
    path,
    originalPath: null,
    kind: "modified",
    staged: false,
    unstaged: true,
    conflicted: false,
    binary: false,
    old: null,
    new: null,
    oldPath: null,
    newPath: null,
    status: ".M",
    indexStatus: ".",
    worktreeStatus: "M",
    additions: 1,
    deletions: 0
  };
}
