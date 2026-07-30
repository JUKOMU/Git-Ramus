import type {
  TransportKind,
  TransportProfileCreateRequest,
  TransportProfileSummary,
  TransportProfileUpdateRequest
} from "@git-ramus/contracts";
import { useState } from "react";

export type TransportProfileMutation =
  TransportProfileCreateRequest | TransportProfileUpdateRequest;

interface TransportProfileFormProps {
  kind: TransportKind;
  profile: TransportProfileSummary | null;
  busy: boolean;
  onSubmit(request: TransportProfileMutation): void;
  onCancel(): void;
}

export function TransportProfileForm({
  kind,
  profile,
  busy,
  onSubmit,
  onCancel
}: TransportProfileFormProps) {
  const [displayName, setDisplayName] = useState(profile?.displayName ?? "");
  const [httpsUsername, setHttpsUsername] = useState(profile?.httpsUsername ?? "");
  const [identitiesOnly, setIdentitiesOnly] = useState(profile?.sshIdentitiesOnly ?? true);
  const valid =
    displayName.trim().length > 0 && (kind === "ssh" || httpsUsername.trim().length > 0);

  const submitHttps = () => {
    if (!valid || busy) return;
    const common = {
      kind: "https" as const,
      displayName: displayName.trim(),
      username: httpsUsername.trim(),
      useHttpPath: true as const
    };
    onSubmit(profile === null ? common : { profileId: profile.id, ...common });
  };

  const submitSsh = (sshKeyAction: "keep" | "selectFile") => {
    if (!valid || busy || (profile === null && sshKeyAction !== "selectFile")) return;
    const common = {
      kind: "ssh" as const,
      displayName: displayName.trim(),
      identitiesOnly
    };
    onSubmit(
      profile === null
        ? { ...common, sshKeyAction: "selectFile" }
        : { profileId: profile.id, ...common, sshKeyAction }
    );
  };

  return (
    <section className="card transport-profile-editor" aria-labelledby="transport-editor-title">
      <header>
        <div>
          <p className="eyebrow">{kind.toUpperCase()} transport</p>
          <h3 id="transport-editor-title">
            {profile === null ? `New ${kind.toUpperCase()} profile` : `Edit ${profile.displayName}`}
          </h3>
        </div>
      </header>
      <form>
        <div className="form-grid transport-profile-form-grid">
          <label>
            Profile name
            <input
              aria-label="Profile name"
              required
              maxLength={128}
              disabled={busy}
              value={displayName}
              onChange={(event) => setDisplayName(event.target.value)}
            />
          </label>
          {kind === "https" ? (
            <label>
              HTTPS username
              <input
                aria-label="HTTPS username"
                required
                maxLength={256}
                disabled={busy}
                value={httpsUsername}
                onChange={(event) => setHttpsUsername(event.target.value)}
              />
            </label>
          ) : (
            <label className="checkbox-label transport-identities-only">
              <input
                type="checkbox"
                checked={identitiesOnly}
                disabled={busy}
                onChange={(event) => setIdentitiesOnly(event.target.checked)}
              />
              Use only the selected SSH identity
            </label>
          )}
        </div>
        {kind === "https" ? (
          <p className="muted transport-profile-guidance">
            Credential lookup is isolated by full HTTPS repository path. This protection remains
            enabled for every profile.
          </p>
        ) : (
          <p className="muted transport-profile-guidance">
            The trusted host file picker stores the private key location. Plugins receive only its
            filename and availability status.
          </p>
        )}
        <div className="button-row transport-profile-actions">
          {kind === "https" || profile !== null ? (
            <button
              type="button"
              disabled={busy || !valid}
              onClick={(event) => {
                if (event.currentTarget.form?.reportValidity() === false) return;
                if (kind === "https") submitHttps();
                else submitSsh("keep");
              }}
            >
              {busy ? "Saving profile…" : "Save profile"}
            </button>
          ) : null}
          {kind === "ssh" ? (
            <button type="button" disabled={busy || !valid} onClick={() => submitSsh("selectFile")}>
              {profile === null ? "Choose private key" : "Choose another private key"}
            </button>
          ) : null}
          <button className="button-link" type="button" disabled={busy} onClick={onCancel}>
            Cancel editing
          </button>
        </div>
      </form>
    </section>
  );
}
