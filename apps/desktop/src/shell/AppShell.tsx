import type {
  Job,
  PluginDescriptor,
  ThemeCatalog,
  ThemeDefinition,
  ThemeState
} from "@git-ramus/contracts";
import type { CSSProperties, ReactNode } from "react";
import { transportPromptBroker } from "../git-transport/promptBroker";
import { TransportConfirmationDialog } from "../git-transport/TransportConfirmationDialog";
import type { HostApi } from "../lib/hostApi";
import { providerAccessBroker, providerCredentialBroker } from "../providers/promptBroker";
import { ProviderAccessDialog } from "../providers/ProviderAccessDialog";
import { ProviderCredentialDialog } from "../providers/ProviderCredentialDialog";
import { TaskCenter } from "./TaskCenter";

interface AppShellProps {
  version: string | null;
  plugins: PluginDescriptor[];
  selectedPluginId: string | null;
  selectedRoute: string | null;
  jobs: Job[];
  hostApi: HostApi;
  themeCatalog: ThemeCatalog;
  themeState: ThemeState | null;
  themeActivationPending: boolean;
  onActivateTheme(themeId: string): void;
  onSelectPlugin(pluginId: string, route: string): void;
  children: ReactNode;
}

export function AppShell(props: AppShellProps) {
  const density = props.themeState?.theme.density ?? "comfortable";
  return (
    <div
      className={`app-shell density-${density}`}
      data-testid="app-shell"
      data-theme-id={props.themeState?.activeThemeId}
      data-theme-density={density}
      style={themeVariables(props.themeState?.theme ?? null)}
    >
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
        <div className="theme-control">
          <label htmlFor="theme-selector">Theme</label>
          <select
            id="theme-selector"
            aria-label="Theme"
            aria-busy={props.themeActivationPending}
            value={props.themeState?.activeThemeId ?? ""}
            disabled={
              props.themeActivationPending ||
              props.themeState === null ||
              props.themeCatalog.themes.length === 0
            }
            onChange={(event) => props.onActivateTheme(event.currentTarget.value)}
          >
            {props.themeState === null ? <option value="">Theme loading</option> : null}
            {props.themeCatalog.themes.map((theme) => (
              <option key={theme.themeId} value={theme.themeId}>
                {theme.name}
              </option>
            ))}
          </select>
        </div>
        <div className="host-version">
          {props.version === null ? "Host loading" : `Host ${props.version}`}
        </div>
      </aside>
      <main className="workspace">
        {props.children}
        <ProviderCredentialDialog broker={providerCredentialBroker} />
        <ProviderAccessDialog broker={providerAccessBroker} />
        <TransportConfirmationDialog broker={transportPromptBroker} />
      </main>
      <TaskCenter jobs={props.jobs} hostApi={props.hostApi} />
    </div>
  );
}

type ThemeVariables = CSSProperties & Record<`--gr-${string}`, string>;

const TOKEN_KEYS = {
  colors: [
    "background",
    "surface",
    "surfaceRaised",
    "text",
    "textMuted",
    "border",
    "primary",
    "secondary",
    "accent",
    "success",
    "warning",
    "danger",
    "focusRing"
  ],
  typography: ["fontFamily", "fontSize", "lineHeight", "fontWeight", "letterSpacing"],
  spacing: ["unit", "xs", "sm", "md", "lg", "xl"],
  shape: ["radius", "radiusSm", "radiusMd", "radiusLg"],
  elevation: ["none", "sm", "md", "lg", "level1", "level2", "level3"],
  motion: ["durationFast", "durationNormal", "durationSlow", "easing"]
} as const;

const LENGTH_TOKENS = new Set([
  "typography.fontSize",
  "typography.letterSpacing",
  ...TOKEN_KEYS.spacing.map((key) => `spacing.${key}`),
  ...TOKEN_KEYS.shape.map((key) => `shape.${key}`)
]);

function themeVariables(theme: ThemeDefinition | null): ThemeVariables {
  const variables: ThemeVariables = {};
  if (theme === null) return variables;
  for (const [group, keys] of Object.entries(TOKEN_KEYS)) {
    const values = theme[group as keyof typeof TOKEN_KEYS] as
      Record<string, string | number | undefined> | undefined;
    if (values === undefined) continue;
    for (const key of keys) {
      const value = values[key];
      if (value === undefined) continue;
      const token = `${group}.${key}`;
      variables[`--gr-${group}-${key}`] =
        typeof value === "number" && LENGTH_TOKENS.has(token) ? `${value}px` : String(value);
    }
  }
  if (theme.density !== undefined) {
    variables["--gr-density"] = theme.density;
  }
  return variables;
}
