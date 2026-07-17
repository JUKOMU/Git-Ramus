import { invoke } from "@tauri-apps/api/core";

export interface AppInfo {
  name: string;
  version: string;
}

export interface HostApi {
  getAppInfo(): Promise<AppInfo>;
}

export const tauriHostApi: HostApi = {
  getAppInfo: () => invoke<AppInfo>("get_app_info")
};
