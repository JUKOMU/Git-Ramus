import TauriWorkerService, { launcher as TauriLaunchService } from "@wdio/tauri-service";

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

export const launcher = TauriLaunchService;
