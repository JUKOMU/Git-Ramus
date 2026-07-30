import { execFile } from "node:child_process";
import { promisify } from "node:util";
import BasicTauriService, { BasicTauriLaunchService } from "./basic-tauri-service";
import { completeLauncherCleanup } from "./launcher-cleanup";

const run = promisify(execFile);

export default BasicTauriService;

export class ExternalTauriLaunchService extends BasicTauriLaunchService {
  override async onComplete(
    exitCode: Parameters<BasicTauriLaunchService["onComplete"]>[0],
    config: Parameters<BasicTauriLaunchService["onComplete"]>[1],
    capabilities: Parameters<BasicTauriLaunchService["onComplete"]>[2]
  ): Promise<void> {
    await completeLauncherCleanup(
      async () => {
        if (process.platform !== "win32") return;
        const driverPool = (
          this as unknown as {
            driverPool?: { getRunningPids(): number[] };
          }
        ).driverPool;
        const pids = driverPool?.getRunningPids() ?? [];
        await Promise.all(pids.map(terminateWindowsProcessTree));
      },
      () => super.onComplete(exitCode, config, capabilities)
    );
  }
}

export const launcher = ExternalTauriLaunchService;

async function terminateWindowsProcessTree(pid: number): Promise<void> {
  try {
    await run("taskkill", ["/PID", String(pid), "/T", "/F"], { windowsHide: true });
  } catch (error: unknown) {
    if (processIsMissing(pid)) return;
    throw error;
  }
}

function processIsMissing(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return false;
  } catch (error: unknown) {
    return error instanceof Error && "code" in error && error.code === "ESRCH";
  }
}
