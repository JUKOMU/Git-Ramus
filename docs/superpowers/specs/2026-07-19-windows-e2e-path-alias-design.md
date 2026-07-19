# Windows E2E Path Alias Fix Design

## Context

The Windows GitHub Actions runner exposes its temporary directory through the DOS 8.3 alias
`C:\Users\RUNNER~1\...`. The Rust host canonicalizes the same directory to
`C:\Users\runneradmin\...` before returning `e2e_app_data_paths`. The current E2E assertion uses
lexically normalized string equality, so it rejects two paths that identify the same directory.

The WebDriver binary-permission and missing-`tauri-driver` diagnostics are unrelated: the embedded
driver starts successfully, and the failure occurs later in `assertIsolatedAppData`.

## Chosen approach

Add an asynchronous helper for existing E2E paths that resolves filesystem identity with Node's
`realpath`, then removes the Windows extended-path prefix through the existing lexical normalizer.
Use that helper only when comparing the host-returned app-data root and database path with the
launcher-owned expected paths.

Keep `normalizeE2eFsPath` unchanged. Cleanup and profile-reuse guards must continue inspecting the
original lexical path and `lstat` result so a symlink or junction cannot be followed before the
guard decides whether deletion is safe.

## Data flow

1. The WDIO launcher creates an isolated temporary profile and passes its path through
   `GIT_RAMUS_WDIO_PROFILE_ROOT`.
2. The Rust host validates and canonicalizes that profile, then returns its root and database path
   through the debug-only `e2e_app_data_paths` command.
3. The E2E assertion resolves both returned and expected existing paths through the new physical
   canonicalizer.
4. Equality is checked only after canonicalization; the database is still required to be a real
   file.

## Error handling and security

- A missing or inaccessible path remains a hard E2E failure because `realpath` rejects.
- The assertion does not fall back to case-insensitive, prefix, or basename comparison.
- Release builds remain unchanged because the host command stays behind the existing debug/E2E
  compilation boundary.
- Cleanup functions retain their current direct-child, prefix, directory, and symlink checks.

## Verification

- Add a focused regression test that creates a physical temporary directory and a filesystem alias
  (a Windows junction or Unix directory symlink), proving both canonicalize to the same path.
- Preserve and rerun the cleanup safety tests, including nested-target rejection.
- Run the focused E2E helper tests in RED and GREEN phases.
- Rebuild the debug E2E application and run both native E2E specs.
- Run the repository-wide format, lint, typecheck, unit tests, and diff checks before committing and
  pushing `main`.

## Non-goals

- Suppressing WDIO diagnostic warnings.
- Changing application data storage or release behavior.
- Weakening filesystem cleanup checks.
- Adding Windows-only native path APIs.
