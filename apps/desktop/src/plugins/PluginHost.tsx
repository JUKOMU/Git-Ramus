import type { PluginDescriptor, ThemeDefinition } from "@git-ramus/contracts";
import type { HostApi } from "../lib/hostApi";
import { PluginFrame } from "./PluginFrame";

interface PluginHostProps {
  descriptor: PluginDescriptor | null;
  hostApi: HostApi;
  route?: string;
  theme?: ThemeDefinition | null;
  onRouteReady?(pluginId: string, route: string): void;
}

export function PluginHost({
  descriptor,
  hostApi,
  route = "/",
  theme = null,
  onRouteReady
}: PluginHostProps) {
  if (descriptor === null) {
    return (
      <section className="empty-state">
        <h2>Foundation ready</h2>
        <p>Select a bundled plugin from the navigation.</p>
      </section>
    );
  }
  if (descriptor.uiUrl === null) {
    return (
      <section className="empty-state">
        <h2>Plugin has no user interface</h2>
        <p>This built-in plugin contributes a trusted backend capability.</p>
      </section>
    );
  }
  const uiDescriptor: PluginDescriptor & { uiUrl: string } = {
    ...descriptor,
    uiUrl: descriptor.uiUrl
  };
  return (
    <PluginFrame
      key={`${descriptor.manifest.id}:${route}`}
      descriptor={uiDescriptor}
      hostApi={hostApi}
      route={route}
      theme={theme}
      {...(onRouteReady === undefined ? {} : { onRouteReady })}
    />
  );
}
