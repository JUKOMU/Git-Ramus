# Git Service Race and Count Invariants Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Make project scans race-free with root mutations, repository discovery idempotent under concurrency, and scan counters internally consistent.

**Architecture:** Add a per-project mutex map beside the existing per-repository write locks and hold a project guard across each scan, update, or deletion. Resolve canonical repository identity in one SQLite transaction. Keep refresh failures attached to repository records while reporting discovery/preparation failures separately.

**Tech Stack:** Rust 2024, `std::sync`, rusqlite/SQLite, Cargo tests.

**Status:** Completed on `main`. The project-lock, atomic repository creation, and counter invariants were delivered by the Git Client consistency fixes ending in `dcbaeef` and remain covered by the current Rust integration suite.

---

### Task 1: Serialize project scans and mutations

**Files:**
- Modify: `apps/desktop/src-tauri/src/git/service.rs`
- Test: `apps/desktop/src-tauri/tests/git_service_integration.rs`

- [x] **Step 1: Write the failing test**

Use a blocking `GitRunner` to stop a scan during its config query. Start a root update concurrently, require its completion channel to time out while the scan is blocked, release the runner, then require both calls to finish and the old project relationship to be empty.

```rust
assert!(matches!(updated.recv_timeout(Duration::from_millis(100)), Err(RecvTimeoutError::Timeout)));
release.send(()).unwrap();
scan.join().unwrap().unwrap();
update.join().unwrap().unwrap();
assert_eq!(service.get_overview_for_project(&project.id).unwrap().repository_count, 0);
```

- [x] **Step 2: Run RED**

Run `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --test git_service_integration project_root_update_waits_for_scan_and_clears_relationships -- --nocapture` and confirm the update completes before the blocked scan is released.

- [x] **Step 3: Implement the lock**

Add `project_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>`, initialize it in the constructor, and add:

```rust
fn project_lock(&self, project_id: &str) -> Arc<Mutex<()>> {
    let mut locks = self.project_locks.lock().expect("project lock map is not poisoned");
    locks.entry(project_id.to_owned()).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
}
```

Acquire this guard before reading the project in `scan_project_with_progress` and `update_project`, and before both delete methods. Do not acquire it in the `scan_project` wrapper. Add an internal `Arc::ptr_eq` test proving different project IDs map to different mutexes.

- [x] **Step 4: Run GREEN**

Re-run both focused lock tests and confirm they pass.

### Task 2: Make canonical repository creation atomic

**Files:**
- Modify: `apps/desktop/src-tauri/src/git/repository.rs`
- Modify: `apps/desktop/src-tauri/src/git/service.rs`
- Test: `apps/desktop/src-tauri/src/git/mod.rs`

- [x] **Step 1: Write and run the failing test**

Start eight barrier-synchronized threads with distinct candidate IDs and one canonical path. Call the wished-for `get_or_create` API and confirm RED because the method does not exist.

- [x] **Step 2: Implement one-transaction get-or-create**

```rust
pub fn get_or_create(&self, repository: &Repository) -> Result<Repository, AppError> {
    self.db.with_transaction(|transaction| {
        transaction.execute(
            "INSERT INTO repositories(id,canonical_path,display_name,kind,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(canonical_path) DO NOTHING",
            params![repository.id, repository.canonical_path, repository.display_name, repository.kind.as_str(), repository.created_at.to_rfc3339(), repository.updated_at.to_rfc3339()],
        )?;
        transaction.query_row(
            "SELECT id,canonical_path,display_name,kind,created_at,updated_at FROM repositories WHERE canonical_path=?1",
            [&repository.canonical_path],
            map_repo,
        )
    })
}
```

Change `ensure_repository` to construct one candidate and return `get_or_create`. Re-run the test and require one returned ID and one database row.

### Task 3: Separate refresh and discovery failure counts

**Files:**
- Modify: `apps/desktop/src-tauri/src/git/service.rs`
- Test: `apps/desktop/src-tauri/tests/git_service_integration.rs`

- [x] **Step 1: Write and run failing invariants**

For a refresh failure require `total=1, completed=0, failed=1, discovery_failed=0`. For an outer-repository discovery failure plus one successful nested repository require `total=1, completed=1, failed=0, discovery_failed=1`. In every case require `total == completed + failed` and every progress total equals the result total.

- [x] **Step 2: Implement split counters**

Add `discovery_failed` to `ScanProjectResult`. Capture discovery/preparation failure count before appending refresh failures, then calculate:

```rust
let total = records.len();
let completed = records.iter().filter(|record| record.error.is_none()).count();
let failed = records.iter().filter(|record| record.error.is_some()).count();
debug_assert_eq!(total, completed + failed);
```

Keep every detail in `failures`, set all `progress.total` values to `total`, and preserve existing relationships skipped only because of excludes. Re-run the focused count tests and confirm GREEN.

### Task 4: Verify and deliver

**Files:**
- Verify every modified Rust and plan file.

- [x] **Step 1: Run focused security and consistency regressions**

Run the untrusted filter/diff/fsmonitor, root rollback, XY mapping, and refresh persistence tests from commits `2c54ae9` and `ec8d283`.

- [x] **Step 2: Run full verification**

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets
git diff --check
```

- [x] **Step 3: Commit**

Stage the plan, service, repositories, and tests, then commit with `fix: serialize project scans and repository discovery`.
