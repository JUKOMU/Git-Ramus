import TauriWorkerService, { launcher as TauriLaunchService } from "@wdio/tauri-service";
import { cleanupOwnedE2eAppDataProfile } from "./app-data-profile";
import { completeLauncherCleanup } from "./launcher-cleanup";

/**
 * The embedded WebDriver server is deliberately HTTP-only. The stock service's
 * optional auto-focus hook calls the richer `tauri-plugin-wdio` IPC bridge, so
 * this E2E service keeps the native launcher/worker setup while leaving focus
 * management to the single-window test.
 */
export default class BasicTauriService extends TauriWorkerService {
  override async beforeCommand(): Promise<void> {
    return;
  }

  override async afterSession(): Promise<void> {
    // The stock cleanup also tries to restore mock state through the richer
    // frontend bridge. WDIO owns the session lifecycle for this basic journey.
    return;
  }
}

export class BasicTauriLaunchService extends TauriLaunchService {
  override async onComplete(
    exitCode: Parameters<TauriLaunchService["onComplete"]>[0],
    config: Parameters<TauriLaunchService["onComplete"]>[1],
    capabilities: Parameters<TauriLaunchService["onComplete"]>[2]
  ): Promise<void> {
    await completeLauncherCleanup(
      () => super.onComplete(exitCode, config, capabilities),
      () => cleanupOwnedE2eAppDataProfile()
    );
  }
}

export const launcher = BasicTauriLaunchService;
