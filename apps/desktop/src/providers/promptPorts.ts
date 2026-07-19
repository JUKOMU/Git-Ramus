import type { ErrorEnvelope, ProviderAuthorizedAccount } from "@git-ramus/contracts";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

export interface ProviderCredentialPromptRequest {
  providerLabel: string;
  accountLabel: string | null;
  purpose: "connect" | "rotate";
}

export interface ProviderAccessPromptRequest {
  pluginId: string;
  accounts: ProviderAuthorizedAccount[];
}

export interface ProviderPromptPort {
  requestCredential(request: ProviderCredentialPromptRequest): Promise<string | null>;
  requestAccountAccess(request: ProviderAccessPromptRequest): Promise<string[] | null>;
}

export interface HostFileSelectionPort {
  selectCertificate(): Promise<string | null>;
}

export const unavailableProviderPromptPort: ProviderPromptPort = {
  async requestCredential() {
    throw promptUnavailableError();
  },
  async requestAccountAccess() {
    throw promptUnavailableError();
  }
};

export const nativeCertificateFileSelectionPort: HostFileSelectionPort = {
  async selectCertificate() {
    const selected = await openDialog({
      multiple: false,
      directory: false,
      title: "Choose a trusted CA certificate",
      filters: [
        {
          name: "Certificates",
          extensions: ["pem", "crt", "cer"]
        }
      ]
    });
    if (selected === null) return null;
    if (Array.isArray(selected) || selected.length === 0) {
      throw new Error("Native certificate selection returned an invalid path");
    }
    return selected;
  }
};

function promptUnavailableError(): ErrorEnvelope {
  return {
    code: "provider.prompt-unavailable",
    category: "userActionRequired",
    message: "Provider prompt is unavailable",
    operationId: null,
    pluginId: null,
    resourceId: null,
    failedStep: "provider.prompt",
    retryable: false,
    retryAfterMs: null,
    recoveryActions: [],
    details: null
  };
}
