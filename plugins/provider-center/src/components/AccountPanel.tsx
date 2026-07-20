import type {
  ErrorEnvelope,
  ProviderAccountDeletionImpact,
  ProviderAccountSummary,
  ProviderInstance
} from "@git-ramus/contracts";
import { useState } from "react";
import type { ProviderCenterApi } from "../api";
import { normalizeError } from "../api";
import { ErrorNotice, StatusBadge } from "./InstancePanel";

interface AccountPanelProps {
  api: ProviderCenterApi;
  instance: ProviderInstance | null;
  accounts: ProviderAccountSummary[];
  selectedAccountId: string | null;
  onSelect(accountId: string): void;
  onRefresh(): Promise<void>;
}

export function AccountPanel({
  api,
  instance,
  accounts,
  selectedAccountId,
  onSelect,
  onRefresh
}: AccountPanelProps) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<ErrorEnvelope | null>(null);
  const [impact, setImpact] = useState<ProviderAccountDeletionImpact | null>(null);
  const [resolution, setResolution] = useState<"reassign" | "inherit" | "unbind" | null>(null);
  const [reassignId, setReassignId] = useState("");
  const [newDefaultId, setNewDefaultId] = useState("");

  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (cause) {
      setError(normalizeError(cause, "Unable to update Provider accounts"));
    } finally {
      setBusy(false);
    }
  };

  if (instance === null) {
    return (
      <section className="panel" aria-labelledby="accounts-heading">
        <h2 id="accounts-heading">Accounts</h2>
        <p className="muted">Select a Provider instance to manage accounts.</p>
      </section>
    );
  }

  const selected = accounts.find(({ id }) => id === selectedAccountId) ?? null;
  const siblings = accounts.filter(({ id }) => id !== selected?.id);
  const canDelete =
    impact !== null &&
    resolution !== null &&
    (resolution !== "reassign" || reassignId.length > 0) &&
    (!impact.requiresNewDefault || newDefaultId.length > 0);

  return (
    <section className="panel" aria-labelledby="accounts-heading">
      <header className="panel-heading">
        <div>
          <p className="eyebrow">Credentials</p>
          <h2 id="accounts-heading">Accounts</h2>
        </div>
        <button
          type="button"
          disabled={busy}
          onClick={() =>
            void run(async () => {
              const connected = await api.connectAccount(instance.id);
              await onRefresh();
              if (connected !== null) onSelect(connected.id);
            })
          }
        >
          Connect account
        </button>
      </header>
      {error === null ? null : <ErrorNotice error={error} />}
      <p className="muted">PAT entry is handled by the trusted host prompt.</p>
      <div className="account-list" role="list" aria-label="Provider accounts">
        {accounts.length === 0 ? <p className="muted">No accounts connected.</p> : null}
        {accounts.map((account) => (
          <div
            className={account.id === selectedAccountId ? "account-card selected" : "account-card"}
            key={account.id}
            role="listitem"
          >
            <button
              type="button"
              className="account-select"
              onClick={() => onSelect(account.id)}
              aria-pressed={account.id === selectedAccountId}
            >
              <span>
                <strong>{account.displayName ?? account.username}</strong>
                <small>{account.username}</small>
              </span>
              <span className="account-state">
                {account.isDefault ? <span className="default-badge">Default</span> : null}
                <StatusBadge status={account.status} />
              </span>
            </button>
            <div className="button-row account-actions">
              <button
                type="button"
                disabled={busy}
                onClick={() =>
                  void run(async () => {
                    await api.validateAccount(account.id);
                    await onRefresh();
                  })
                }
              >
                Validate
              </button>
              <button
                type="button"
                disabled={busy}
                onClick={() =>
                  void run(async () => {
                    const rotated = await api.rotateAccount(account.id);
                    await onRefresh();
                    if (rotated !== null) onSelect(rotated.id);
                  })
                }
              >
                Rotate
              </button>
              {!account.isDefault ? (
                <button
                  type="button"
                  disabled={busy}
                  onClick={() =>
                    void run(async () => {
                      await api.setDefaultAccount(instance.id, account.id);
                      await onRefresh();
                    })
                  }
                >
                  Set default
                </button>
              ) : null}
              <button
                type="button"
                className="danger-button"
                disabled={busy}
                onClick={() =>
                  void run(async () => {
                    const next = await api.getAccountDeletionImpact(account.id);
                    setImpact(next);
                    setResolution(null);
                    setReassignId("");
                    setNewDefaultId("");
                  })
                }
              >
                Delete…
              </button>
            </div>
          </div>
        ))}
      </div>

      {impact === null ? null : (
        <div
          className="deletion-panel"
          role="dialog"
          aria-labelledby="account-delete-heading"
          aria-modal="false"
        >
          <h3 id="account-delete-heading">Delete account</h3>
          <p>
            This account has {impact.explicitBindingCount} explicit and{" "}
            {impact.inheritedBindingCount} inherited bindings.
          </p>
          <fieldset>
            <legend>Binding resolution</legend>
            <label className="check-row">
              <input
                type="radio"
                name="account-resolution"
                value="reassign"
                checked={resolution === "reassign"}
                onChange={() => setResolution("reassign")}
              />{" "}
              Reassign bindings
            </label>
            {resolution === "reassign" ? (
              <label>
                Reassign to
                <select
                  value={reassignId}
                  onChange={(event) => setReassignId(event.currentTarget.value)}
                >
                  <option value="">Choose an account</option>
                  {siblings.map((account) => (
                    <option value={account.id} key={account.id}>
                      {account.displayName ?? account.username}
                    </option>
                  ))}
                </select>
              </label>
            ) : null}
            <label className="check-row">
              <input
                type="radio"
                name="account-resolution"
                value="inherit"
                checked={resolution === "inherit"}
                onChange={() => setResolution("inherit")}
              />{" "}
              Inherit instance default
            </label>
            <label className="check-row">
              <input
                type="radio"
                name="account-resolution"
                value="unbind"
                checked={resolution === "unbind"}
                onChange={() => setResolution("unbind")}
              />{" "}
              Unbind affected remotes
            </label>
          </fieldset>
          {impact.requiresNewDefault ? (
            <label>
              New default account
              <select
                value={newDefaultId}
                onChange={(event) => setNewDefaultId(event.currentTarget.value)}
              >
                <option value="">Choose a new default</option>
                {siblings.map((account) => (
                  <option value={account.id} key={account.id}>
                    {account.displayName ?? account.username}
                  </option>
                ))}
              </select>
            </label>
          ) : null}
          <div className="button-row">
            <button type="button" onClick={() => setImpact(null)}>
              Cancel
            </button>
            <button
              type="button"
              className="danger-button"
              disabled={!canDelete || busy}
              onClick={() =>
                void run(async () => {
                  const chosenResolution =
                    resolution === "reassign"
                      ? { kind: "reassign" as const, accountId: reassignId }
                      : { kind: resolution! as "inherit" | "unbind" };
                  await api.deleteAccount({
                    accountId: impact.accountId,
                    resolution: chosenResolution,
                    newDefaultAccountId: impact.requiresNewDefault ? newDefaultId : null
                  });
                  setImpact(null);
                  await onRefresh();
                })
              }
            >
              Delete account
            </button>
          </div>
        </div>
      )}
    </section>
  );
}
