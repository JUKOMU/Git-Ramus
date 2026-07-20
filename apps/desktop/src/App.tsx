import {
  themeStateSchema,
  type Job,
  type PluginDescriptor,
  type ThemeCatalog,
  type ThemeState
} from "@git-ramus/contracts";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { HostApi } from "./lib/hostApi";
import { tauriHostApi } from "./lib/hostApi";
import { providerAccessBroker, providerCredentialBroker } from "./providers/promptBroker";
import { ProviderAccessDialog } from "./providers/ProviderAccessDialog";
import { ProviderCredentialDialog } from "./providers/ProviderCredentialDialog";
import { PluginHost } from "./plugins/PluginHost";
import { AppShell } from "./shell/AppShell";

interface AppProps {
  hostApi?: HostApi;
}

interface PluginSelection {
  pluginId: string;
  route: string;
}

export function App({ hostApi = tauriHostApi }: AppProps) {
  const [version, setVersion] = useState<string | null>(null);
  const [plugins, setPlugins] = useState<PluginDescriptor[]>([]);
  const [jobs, setJobs] = useState<Job[]>([]);
  const [selection, setSelection] = useState<PluginSelection | null>(null);
  const [themeCatalog, setThemeCatalog] = useState<ThemeCatalog>({ themes: [] });
  const [themeState, setThemeState] = useState<ThemeState | null>(null);
  const [themeActivationPending, setThemeActivationPending] = useState(false);
  const activationTail = useRef<Promise<void>>(Promise.resolve());
  const activationGeneration = useRef(0);
  const activationPending = useRef(false);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      activationGeneration.current += 1;
    };
  }, []);

  useEffect(() => {
    let active = true;
    void Promise.all([
      hostApi.getAppInfo(),
      hostApi.listPlugins(),
      hostApi.listJobs(),
      hostApi.listThemes(),
      hostApi.currentTheme()
    ]).then(([info, loadedPlugins, loadedJobs, loadedThemes, loadedTheme]) => {
      if (active) {
        setVersion(info.version);
        setPlugins(loadedPlugins);
        setJobs(loadedJobs);
        setThemeCatalog(loadedThemes);
        setThemeState(themeStateSchema.parse(loadedTheme));
      }
    });
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
    void listen<ThemeState>("theme://changed", (event) => {
      const parsed = themeStateSchema.safeParse(event.payload);
      if (parsed.success && !activationPending.current) {
        setThemeState(parsed.data);
      }
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

  const activateTheme = useCallback(
    (themeId: string) => {
      const generation = ++activationGeneration.current;
      activationPending.current = true;
      setThemeActivationPending(true);

      const run = async () => {
        if (!mounted.current) return;
        try {
          const activated = await hostApi.activateTheme({ themeId });
          const parsed = themeStateSchema.parse(activated);
          if (mounted.current && generation === activationGeneration.current) {
            setThemeState(parsed);
          }
        } catch {
          if (mounted.current && generation === activationGeneration.current) {
            try {
              const authoritative = themeStateSchema.parse(await hostApi.currentTheme());
              if (mounted.current && generation === activationGeneration.current) {
                setThemeState(authoritative);
              }
            } catch {
              // Preserve the last confirmed theme when reconciliation also fails.
            }
          }
        } finally {
          if (mounted.current && generation === activationGeneration.current) {
            activationPending.current = false;
            setThemeActivationPending(false);
          }
        }
      };

      activationTail.current = activationTail.current.then(run, run);
    },
    [hostApi]
  );

  return (
    <>
      <AppShell
        version={version}
        plugins={plugins}
        selectedPluginId={selection?.pluginId ?? null}
        selectedRoute={selection?.route ?? null}
        jobs={jobs}
        hostApi={hostApi}
        themeCatalog={themeCatalog}
        themeState={themeState}
        themeActivationPending={themeActivationPending}
        onActivateTheme={activateTheme}
        onSelectPlugin={(pluginId, route) => setSelection({ pluginId, route })}
      >
        <PluginHost
          descriptor={selected}
          hostApi={hostApi}
          route={selection?.route ?? "/"}
          theme={themeState?.theme ?? null}
        />
      </AppShell>
      <ProviderCredentialDialog broker={providerCredentialBroker} />
      <ProviderAccessDialog broker={providerAccessBroker} />
    </>
  );
}

function upsertJob(jobs: Job[], update: Job): Job[] {
  return [update, ...jobs.filter((job) => job.id !== update.id)];
}
