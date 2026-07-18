import type { Job, PluginDescriptor, ThemeDefinition } from "@git-ramus/contracts";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useMemo, useState } from "react";
import type { HostApi } from "./lib/hostApi";
import { tauriHostApi } from "./lib/hostApi";
import { PluginHost } from "./plugins/PluginHost";
import { AppShell } from "./shell/AppShell";

interface AppProps {
  hostApi?: HostApi;
  theme?: ThemeDefinition | null;
}

interface PluginSelection {
  pluginId: string;
  route: string;
}

export function App({ hostApi = tauriHostApi, theme = null }: AppProps) {
  const [version, setVersion] = useState<string | null>(null);
  const [plugins, setPlugins] = useState<PluginDescriptor[]>([]);
  const [jobs, setJobs] = useState<Job[]>([]);
  const [selection, setSelection] = useState<PluginSelection | null>(null);

  useEffect(() => {
    let active = true;
    void Promise.all([hostApi.getAppInfo(), hostApi.listPlugins(), hostApi.listJobs()]).then(
      ([info, loadedPlugins, loadedJobs]) => {
        if (active) {
          setVersion(info.version);
          setPlugins(loadedPlugins);
          setJobs(loadedJobs);
        }
      }
    );
    return () => {
      active = false;
    };
  }, [hostApi]);

  useEffect(() => {
    if (hostApi !== tauriHostApi) {
      return;
    }
    let active = true;
    let unlisten: (() => void) | null = null;
    void listen<Job>("job://updated", (event) => {
      setJobs((current) => upsertJob(current, event.payload));
    }).then((dispose) => {
      if (active) {
        unlisten = dispose;
      } else {
        dispose();
      }
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [hostApi]);

  const selected = useMemo(
    () => plugins.find((plugin) => plugin.manifest.id === selection?.pluginId) ?? null,
    [plugins, selection?.pluginId]
  );

  return (
    <AppShell
      version={version}
      plugins={plugins}
      selectedPluginId={selection?.pluginId ?? null}
      selectedRoute={selection?.route ?? null}
      jobs={jobs}
      hostApi={hostApi}
      onSelectPlugin={(pluginId, route) => setSelection({ pluginId, route })}
    >
      <PluginHost
        descriptor={selected}
        hostApi={hostApi}
        route={selection?.route ?? "/"}
        theme={theme}
      />
    </AppShell>
  );
}

function upsertJob(jobs: Job[], update: Job): Job[] {
  return [update, ...jobs.filter((job) => job.id !== update.id)];
}
