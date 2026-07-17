import type { PluginDescriptor } from "@git-ramus/contracts";
import type { HostApi } from "../lib/hostApi";
import { PluginFrame } from "./PluginFrame";

interface PluginHostProps {
  descriptor: PluginDescriptor | null;
  hostApi: HostApi;
}

export function PluginHost({ descriptor, hostApi }: PluginHostProps) {
  if (descriptor === null) {
    return (
      <section className="empty-state">
        <h2>Foundation ready</h2>
        <p>Select a bundled plugin from the navigation.</p>
      </section>
    );
  }
  return <PluginFrame key={descriptor.manifest.id} descriptor={descriptor} hostApi={hostApi} />;
}
