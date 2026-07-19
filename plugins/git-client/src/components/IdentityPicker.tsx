import type { EffectiveIdentity, IdentityProfile } from "@git-ramus/contracts";

interface IdentityPickerProps {
  identities: IdentityProfile[];
  globalIdentityProfileId: string | null;
  effectiveIdentity: EffectiveIdentity | null;
  selectedIdentityProfileId: string | null;
  onChange(profileId: string | null): void;
}

const sourceLabels: Record<EffectiveIdentity["source"], string> = {
  globalProfile: "Global profile",
  repositoryProfile: "Repository profile",
  selectedProfile: "Selected profile",
  externalGlobal: "External global",
  externalLocal: "External local"
};

export function IdentityPicker({
  identities,
  globalIdentityProfileId,
  effectiveIdentity,
  selectedIdentityProfileId,
  onChange
}: IdentityPickerProps) {
  const selectedProfile =
    identities.find((profile) => profile.id === selectedIdentityProfileId) ??
    effectiveIdentity?.profile ??
    null;

  return (
    <section className="identity-picker" aria-label="Commit identity settings">
      <label>
        Commit identity
        <select
          aria-label="Commit identity"
          value={selectedIdentityProfileId ?? ""}
          onChange={(event) => onChange(event.target.value === "" ? null : event.target.value)}
        >
          <option value="">Use effective identity</option>
          {identities.map((profile) => (
            <option key={profile.id} value={profile.id}>
              {profile.displayName} · {profile.userEmail}
            </option>
          ))}
        </select>
      </label>
      <div className="identity-meta">
        <span>
          Effective source: {effectiveIdentity ? sourceLabels[effectiveIdentity.source] : "Loading"}
        </span>
        {selectedProfile?.id === globalIdentityProfileId ? (
          <span className="badge">Global</span>
        ) : null}
        <span>
          {selectedProfile?.signCommits
            ? `Signing enabled · ${(selectedProfile.gpgFormat ?? "configured").toUpperCase()}`
            : "Signing disabled"}
        </span>
      </div>
    </section>
  );
}
