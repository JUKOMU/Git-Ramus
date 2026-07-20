import type { ErrorEnvelope } from "@git-ramus/contracts";
import { invoke } from "@tauri-apps/api/core";

export interface GitTransportPromptRequest {
  pluginId: string;
  operationId: string | null;
  kind: "network" | "sourceTrust" | "replaceConfig";
  operation: "clone" | "fetch" | "pull" | "push" | "bindProfile";
  resourceLabel: string;
}

export interface GitTransportPromptPort {
  confirm(request: GitTransportPromptRequest): Promise<boolean>;
  cancelOperation(pluginId: string, operationId: string): void;
  isOperationCanceled(pluginId: string, operationId: string): boolean;
}

export interface GitTransportFilePort {
  selectDestinationParent(defaultPath?: string): Promise<string | null>;
  selectSshPrivateKey(): Promise<string | null>;
}

const GIT_CLIENT_PLUGIN_ID = "git-ramus.git-client";

export const unavailableGitTransportPromptPort: GitTransportPromptPort = {
  async confirm() {
    throw transportPortError("git.transport.prompt-unavailable", "Git transport prompt");
  },
  cancelOperation() {},
  isOperationCanceled() {
    return false;
  }
};

export const nativeGitTransportFilePort: GitTransportFilePort = {
  async selectDestinationParent(defaultPath) {
    void defaultPath;
    return invokeHostPath("git_transport_select_destination_parent");
  },
  async selectSshPrivateKey() {
    return invokeHostPath("git_transport_select_ssh_key");
  }
};

async function invokeHostPath(command: string): Promise<string | null> {
  const selected = await invoke<unknown>(command, {
    request: { pluginId: GIT_CLIENT_PLUGIN_ID }
  });
  if (selected === null) return null;
  if (typeof selected !== "string") {
    throw new Error("Native Git transport selection returned an invalid path");
  }
  const path = selected;
  if (
    path !== path.trim() ||
    path.length === 0 ||
    path.length > 32_768 ||
    Array.from(path).some((character) => {
      const code = character.charCodeAt(0);
      return code < 0x20 || code === 0x7f;
    }) ||
    !/^(?:[A-Za-z]:[\\/]|\\\\|\/)/u.test(path)
  ) {
    throw new Error("Native Git transport selection returned an invalid path");
  }
  return path;
}

function transportPortError(code: string, subject: string): ErrorEnvelope {
  return {
    code,
    category: "userActionRequired",
    message: `${subject} is unavailable`,
    operationId: null,
    pluginId: null,
    resourceId: null,
    failedStep: "git.transport.prompt",
    retryable: false,
    retryAfterMs: null,
    recoveryActions: [],
    details: null
  };
}
