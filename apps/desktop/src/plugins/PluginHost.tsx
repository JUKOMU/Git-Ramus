import type { PluginDescriptor, ThemeDefinition } from "@git-ramus/contracts";
import type { HostApi } from "../lib/hostApi";
import { PluginFrame } from "./PluginFrame";

interface PluginHostProps {
  descriptor: PluginDescriptor | null;
  hostApi: HostApi;
  route?: string;
  theme?: ThemeDefinition | null;
}

export function PluginHost({ descriptor, hostApi, route = "/", theme = null }: PluginHostProps) {
  if (descriptor === null) {
    return (
      <section className="empty-state">
        <h2>Foundation ready</h2>
        <p>Select a bundled plugin from the navigation.</p>
      </section>
    );
  }
  return (
    <PluginFrame
      key={`${descriptor.manifest.id}:${route}`}
      descriptor={descriptor}
      hostApi={hostApi}
      route={route}
      theme={theme}
    />
  );
}
