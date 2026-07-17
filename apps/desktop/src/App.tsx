import { useEffect, useState } from "react";
import type { HostApi } from "./lib/hostApi";
import { tauriHostApi } from "./lib/hostApi";
import { AppShell } from "./shell/AppShell";

interface AppProps {
  hostApi?: HostApi;
}

export function App({ hostApi = tauriHostApi }: AppProps) {
  const [version, setVersion] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    void hostApi.getAppInfo().then((info) => {
      if (active) {
        setVersion(info.version);
      }
    });
    return () => {
      active = false;
    };
  }, [hostApi]);

  return (
    <AppShell version={version}>
      <section className="empty-state">
        <h2>Foundation ready</h2>
        <p>Bundled plugins will contribute pages here.</p>
      </section>
    </AppShell>
  );
}
