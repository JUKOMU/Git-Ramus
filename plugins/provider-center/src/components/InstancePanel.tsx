import type { ErrorEnvelope, ProviderInstance, ProviderKind } from "@git-ramus/contracts";
import { useEffect, useState } from "react";
import type { ProviderCenterApi } from "../api";
import { normalizeError } from "../api";

interface InstancePanelProps {
  api: ProviderCenterApi;
  instances: ProviderInstance[];
  selectedInstanceId: string | null;
  onSelect(instanceId: string): void;
  onRefresh(preferredInstanceId?: string, expectedInstanceId?: string): Promise<void>;
}

export function InstancePanel({
  api,
  instances,
  selectedInstanceId,
  onSelect,
  onRefresh
}: InstancePanelProps) {
  const selected = instances.find(({ id }) => id === selectedInstanceId) ?? null;
  const [kind, setKind] = useState<ProviderKind>("github");
  const [displayName, setDisplayName] = useState("GitHub");
  const [baseUrl, setBaseUrl] = useState("https://github.com");
  const [customCa, setCustomCa] = useState(false);
  const [editDisplayName, setEditDisplayName] = useState("");
  const [editBaseUrl, setEditBaseUrl] = useState("");
  const [editCustomCaAction, setEditCustomCaAction] = useState<"keep" | "remove" | "selectFile">(
    "keep"
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<ErrorEnvelope | null>(null);

  useEffect(() => {
    void Promise.resolve().then(() => {
      if (kind === "github") {
        setBaseUrl("https://github.com");
        setCustomCa(false);
        if (displayName.trim().length === 0 || displayName === "GitLab") setDisplayName("GitHub");
      } else if (displayName === "GitHub") {
        setDisplayName("GitLab");
        setBaseUrl("https://gitlab.com");
      }
    });
  }, [displayName, kind]);

  useEffect(() => {
    void Promise.resolve().then(() => {
      if (selected === null) {
        setEditDisplayName("");
        setEditBaseUrl("");
        setEditCustomCaAction("keep");
        return;
      }
      setEditDisplayName(selected.displayName);
      setEditBaseUrl(selected.baseUrl);
      setEditCustomCaAction("keep");
    });
  }, [selected]);

  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (cause) {
      setError(normalizeError(cause, "Unable to update Provider instances"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="panel" aria-labelledby="provider-instances-heading">
      <header className="panel-heading">
        <div>
          <p className="eyebrow">Connections</p>
          <h2 id="provider-instances-heading">Provider instances</h2>
        </div>
        <button
          type="button"
          onClick={() => void onRefresh(undefined, selected?.id)}
          disabled={busy}
        >
          Refresh instances
        </button>
      </header>

      {error === null ? null : <ErrorNotice error={error} />}

      <div className="provider-instance-list" role="list" aria-label="Provider instances">
        {instances.length === 0 ? <p className="muted">No Provider instances configured.</p> : null}
        {instances.map((instance) => (
          <button
            type="button"
            role="listitem"
            className={
              instance.id === selectedInstanceId ? "selection-card selected" : "selection-card"
            }
            key={instance.id}
            onClick={() => onSelect(instance.id)}
            aria-pressed={instance.id === selectedInstanceId}
          >
            <span>
              <strong>{instance.displayName}</strong>
              <small>{instance.baseUrl}</small>
            </span>
            <StatusBadge status={instance.providerEnabled ? instance.status : "unavailable"} />
          </button>
        ))}
      </div>

      <form className="form-stack" aria-label="Create Provider instance">
        <h3>Add instance</h3>
        <label>
          Provider type
          <select
            value={kind}
            onChange={(event) => setKind(event.currentTarget.value as ProviderKind)}
          >
            <option value="github">GitHub</option>
            <option value="gitlab">GitLab</option>
          </select>
        </label>
        <label>
          Display name
          <input
            value={displayName}
            onChange={(event) => setDisplayName(event.currentTarget.value)}
            required
          />
        </label>
        <label>
          Base URL
          <input
            type="url"
            value={baseUrl}
            onChange={(event) => setBaseUrl(event.currentTarget.value)}
            readOnly={kind === "github"}
            aria-describedby={kind === "gitlab" ? "gitlab-url-help" : undefined}
            required
          />
        </label>
        {kind === "gitlab" ? (
          <>
            <small id="gitlab-url-help" className="muted">
              Use an HTTPS GitLab cloud or self-managed base URL.
            </small>
            <label className="check-row">
              <input
                type="checkbox"
                checked={customCa}
                onChange={(event) => setCustomCa(event.currentTarget.checked)}
              />
              Select a custom CA certificate
            </label>
          </>
        ) : null}
        <button
          type="button"
          disabled={busy || displayName.trim().length === 0}
          onClick={(event) => {
            if (event.currentTarget.form?.reportValidity() === false) return;
            void run(async () => {
              const created = await api.createInstance({
                providerKind: kind,
                displayName,
                baseUrl,
                customCaAction: kind === "gitlab" && customCa ? "selectFile" : "none"
              });
              if (created !== null) await onRefresh(created.id);
            });
          }}
        >
          Create instance
        </button>
      </form>

      {selected === null ? null : (
        <div className="selected-actions" aria-label="Selected instance actions">
          <h3>{selected.displayName}</h3>
          {!selected.providerEnabled ? (
            <p className="warning-notice">
              Provider adapter disabled. Enable the built-in Provider plugin, then refresh.
            </p>
          ) : null}
          <button
            type="button"
            disabled={busy}
            onClick={() =>
              void run(async () => {
                await api.validateInstance({ instanceId: selected.id });
                await onRefresh(selected.id, selected.id);
              })
            }
          >
            Validate instance
          </button>
          <button
            type="button"
            className="danger-button"
            disabled={busy}
            onClick={() =>
              void run(async () => {
                await api.deleteInstance({ instanceId: selected.id });
                await onRefresh(undefined, selected.id);
              })
            }
          >
            Delete instance
          </button>
          <form className="form-stack" aria-label="Edit Provider instance">
            <h3>Edit instance</h3>
            <label>
              Display name
              <input
                value={editDisplayName}
                onChange={(event) => setEditDisplayName(event.currentTarget.value)}
                required
              />
            </label>
            <label>
              Base URL
              <input
                type="url"
                value={editBaseUrl}
                onChange={(event) => setEditBaseUrl(event.currentTarget.value)}
                readOnly={selected.providerKind === "github"}
                required
              />
            </label>
            {selected.providerKind === "gitlab" ? (
              <label>
                Custom CA
                <select
                  value={editCustomCaAction}
                  onChange={(event) =>
                    setEditCustomCaAction(
                      event.currentTarget.value as "keep" | "remove" | "selectFile"
                    )
                  }
                >
                  <option value="keep">Keep current certificate</option>
                  <option value="remove">Remove certificate</option>
                  <option value="selectFile">Select a replacement certificate</option>
                </select>
              </label>
            ) : null}
            <button
              type="button"
              disabled={
                busy || editDisplayName.trim().length === 0 || editBaseUrl.trim().length === 0
              }
              onClick={(event) => {
                if (event.currentTarget.form?.reportValidity() === false) return;
                void run(async () => {
                  const updated = await api.updateInstance({
                    instanceId: selected.id,
                    displayName: editDisplayName,
                    baseUrl: editBaseUrl,
                    customCaAction: selected.providerKind === "github" ? "keep" : editCustomCaAction
                  });
                  if (updated !== null) await onRefresh(updated.id, selected.id);
                });
              }}
            >
              Save instance
            </button>
          </form>
        </div>
      )}
    </section>
  );
}

export function StatusBadge({ status }: { status: ProviderInstance["status"] }) {
  return <span className={`status-badge status-${status}`}>{statusLabel(status)}</span>;
}

export function ErrorNotice({ error }: { error: ErrorEnvelope }) {
  return (
    <div className="error-notice" role="alert">
      <strong>{error.message}</strong>
      {error.retryAfterMs === null ? null : <span> Retry after {error.retryAfterMs} ms.</span>}
      {error.recoveryActions.length === 0 ? null : (
        <ul>
          {error.recoveryActions.map((action) => (
            <li key={action.id}>{action.label}</li>
          ))}
        </ul>
      )}
    </div>
  );
}

function statusLabel(status: ProviderInstance["status"]): string {
  switch (status) {
    case "connected":
      return "Connected";
    case "actionRequired":
      return "Action required";
    case "rateLimited":
      return "Rate limited";
    case "unavailable":
      return "Unavailable";
  }
}
