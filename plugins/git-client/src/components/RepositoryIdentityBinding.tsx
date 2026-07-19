import type { IdentityProfile } from "@git-ramus/contracts";

interface RepositoryIdentityBindingProps {
  identities: IdentityProfile[];
  boundIdentityProfileId: string | null;
  selectedIdentityProfileId: string | null;
  operation: "bind" | "unbind" | null;
  onSelectionChange(profileId: string | null): void;
  onBind(): void;
  onUnbind(): void;
}

export function RepositoryIdentityBinding({
  identities,
  boundIdentityProfileId,
  selectedIdentityProfileId,
  operation,
  onSelectionChange,
  onBind,
  onUnbind
}: RepositoryIdentityBindingProps) {
  const busy = operation !== null;
  const boundProfile =
    identities.find((identity) => identity.id === boundIdentityProfileId) ?? null;
  const canBind =
    selectedIdentityProfileId !== null &&
    selectedIdentityProfileId !== boundIdentityProfileId &&
    !busy;

  return (
    <section
      className="identity-picker repository-identity-binding"
      aria-label="Repository identity settings"
    >
      <label>
        Repository identity
        <select
          aria-label="Repository identity"
          value={selectedIdentityProfileId ?? ""}
          disabled={busy || identities.length === 0}
          onChange={(event) =>
            onSelectionChange(event.target.value === "" ? null : event.target.value)
          }
        >
          <option value="">No repository binding</option>
          {identities.map((identity) => (
            <option key={identity.id} value={identity.id}>
              {identity.displayName} · {identity.userEmail}
            </option>
          ))}
        </select>
      </label>
      <p className="muted">
        {boundProfile === null
          ? "This repository follows its effective global or external Git identity."
          : `Bound to ${boundProfile.displayName}.`}
      </p>
      <div className="button-row repository-identity-actions">
        <button
          type="button"
          disabled={!canBind}
          aria-label={
            operation === "bind" ? "Binding repository identity…" : "Bind repository identity"
          }
          onClick={onBind}
        >
          {operation === "bind" ? "Binding…" : "Bind"}
        </button>
        <button
          className="button-link"
          type="button"
          disabled={busy || boundIdentityProfileId === null}
          aria-label={
            operation === "unbind" ? "Unbinding repository identity…" : "Unbind repository identity"
          }
          onClick={onUnbind}
        >
          {operation === "unbind" ? "Unbinding…" : "Unbind"}
        </button>
      </div>
    </section>
  );
}
