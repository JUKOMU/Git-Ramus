import type { Job, PluginDescriptor } from "@git-ramus/contracts";
import type { ReactNode } from "react";
import type { HostApi } from "../lib/hostApi";
import { TaskCenter } from "./TaskCenter";

interface AppShellProps {
  version: string | null;
  plugins: PluginDescriptor[];
  selectedPluginId: string | null;
  selectedRoute: string | null;
  jobs: Job[];
  hostApi: HostApi;
  onSelectPlugin(pluginId: string, route: string): void;
  children: ReactNode;
}

export function AppShell(props: AppShellProps) {
  return (
    <div className="app-shell">
      <aside className="sidebar">
        <h1>Git-Ramus</h1>
        <nav aria-label="Primary">
          {props.plugins.flatMap((plugin) =>
            plugin.manifest.contributions.navigation.map((item) => (
              <button
                className="nav-item"
                aria-pressed={
                  props.selectedPluginId === plugin.manifest.id &&
                  props.selectedRoute === item.route
                }
                key={`${plugin.manifest.id}:${item.id}`}
                type="button"
                onClick={() => props.onSelectPlugin(plugin.manifest.id, item.route)}
              >
                {item.label}
              </button>
            ))
          )}
        </nav>
        <div className="host-version">
          {props.version === null ? "Host loading" : `Host ${props.version}`}
        </div>
      </aside>
      <main className="workspace">{props.children}</main>
      <TaskCenter jobs={props.jobs} hostApi={props.hostApi} />
    </div>
  );
}
