import type { ReactNode } from "react";

interface AppShellProps {
  version: string | null;
  children: ReactNode;
}

const primaryItems = ["Overview", "Projects", "Workspaces", "Plugins"];

export function AppShell({ version, children }: AppShellProps) {
  return (
    <div className="app-shell">
      <aside className="sidebar">
        <h1>Git-Ramus</h1>
        <nav aria-label="Primary">
          {primaryItems.map((item) => (
            <button className="nav-item" key={item} type="button">
              {item}
            </button>
          ))}
        </nav>
        <div className="host-version">{version === null ? "Host loading" : `Host ${version}`}</div>
      </aside>
      <main className="workspace">{children}</main>
      <aside className="task-rail" aria-label="Task center">
        <button type="button">Tasks</button>
        <p>No active tasks</p>
      </aside>
    </div>
  );
}
