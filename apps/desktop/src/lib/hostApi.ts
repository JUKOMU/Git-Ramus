import type { Job, PluginDescriptor } from "@git-ramus/contracts";
import { invoke } from "@tauri-apps/api/core";

export interface AppInfo {
  name: string;
  version: string;
}

export interface AuthorizationRequest {
  pluginId: string;
  capability: string;
  resource: string;
}

export interface AuthorizationDecision {
  allowed: boolean;
}

export interface HostApi {
  getAppInfo(): Promise<AppInfo>;
  listPlugins(): Promise<PluginDescriptor[]>;
  listJobs(): Promise<Job[]>;
  authorizePluginCall(request: AuthorizationRequest): Promise<AuthorizationDecision>;
  startEchoJob(pluginId: string, message: string): Promise<Job>;
  cancelJob(jobId: string): Promise<void>;
}

export const tauriHostApi: HostApi = {
  getAppInfo: () => invoke<AppInfo>("get_app_info"),
  listPlugins: () => invoke<PluginDescriptor[]>("list_plugins"),
  listJobs: () => invoke<Job[]>("list_jobs"),
  authorizePluginCall: (request) =>
    invoke<AuthorizationDecision>("authorize_plugin_call", { request }),
  startEchoJob: (pluginId, message) =>
    invoke<Job>("start_echo_job", { request: { pluginId, message } }),
  cancelJob: (jobId) => invoke<void>("cancel_job", { jobId })
};
