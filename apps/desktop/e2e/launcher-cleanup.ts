export async function completeLauncherCleanup(
  stopLauncher: () => Promise<void>,
  cleanupProfile: () => Promise<void>
): Promise<void> {
  const errors: unknown[] = [];
  try {
    await stopLauncher();
  } catch (error: unknown) {
    errors.push(error);
  }
  try {
    await cleanupProfile();
  } catch (error: unknown) {
    errors.push(error);
  }
  if (errors.length > 0) {
    throw new AggregateError(errors, "Tauri E2E launcher cleanup failed");
  }
}
