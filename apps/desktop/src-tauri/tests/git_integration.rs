use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use git_ramus_desktop_lib::error::{AppError, ErrorCategory, ErrorEnvelope};
use git_ramus_desktop_lib::git::engine::{GitCommand, GitRunner, SystemGitRunner};
use git_ramus_desktop_lib::git::model::RepositoryKind;
use git_ramus_desktop_lib::git::parser::{
    ChangeKind, DiffSummary, detect_repository, parse_diff_summary, parse_git_config,
    parse_status_v2,
};

fn status_fixture() -> Vec<u8> {
    let mut bytes = Vec::new();
    for record in [
        b"# branch.oid 0123456789012345678901234567890123456789".as_slice(),
        b"# branch.head main".as_slice(),
        b"# branch.upstream origin/main".as_slice(),
        b"# branch.ab +2 -1".as_slice(),
        b"1 M. N... 100644 100644 100644 abcdef1 abcdef2 staged.txt".as_slice(),
        b"1 .M N... 100644 100644 100644 abcdef1 abcdef3 working file.txt".as_slice(),
        b"2 R. N... 100644 100644 100644 abcdef1 abcdef4 R100 renamed file.txt".as_slice(),
        b"old name with spaces.txt".as_slice(),
        "? 文件.txt".as_bytes(),
        b"u UU N... 100644 100644 100644 100644 aaaaaaa bbbbbbb ccccccc ddddddd conflict.txt"
            .as_slice(),
    ] {
        bytes.extend_from_slice(record);
        bytes.push(0);
    }
    bytes
}

#[test]
fn porcelain_v2_fixture_preserves_paths_and_counts() {
    let snapshot = parse_status_v2(status_fixture()).expect("fixture parses");
    assert_eq!(snapshot.branch.as_deref(), Some("main"));
    assert_eq!(snapshot.upstream.as_deref(), Some("origin/main"));
    assert_eq!(
        snapshot.head_oid.as_deref(),
        Some("0123456789012345678901234567890123456789")
    );
    assert_eq!((snapshot.ahead, snapshot.behind), (2, 1));
    assert_eq!(snapshot.changes.len(), 5);
    assert_eq!(snapshot.staged_count, 3);
    assert_eq!(snapshot.unstaged_count, 2);
    assert_eq!(snapshot.untracked_count, 1);
    assert_eq!(snapshot.conflicted_count, 1);

    let renamed = snapshot
        .changes
        .iter()
        .find(|entry| entry.kind == ChangeKind::Renamed)
        .expect("rename entry");
    assert_eq!(renamed.path, "renamed file.txt");
    assert_eq!(
        renamed.original_path.as_deref(),
        Some("old name with spaces.txt")
    );
    assert!(renamed.staged);
    assert!(!renamed.conflicted);
    assert_eq!(renamed.old.as_deref(), Some("old name with spaces.txt"));
    assert_eq!(renamed.new.as_deref(), Some("renamed file.txt"));

    let untracked = snapshot
        .changes
        .iter()
        .find(|entry| entry.kind == ChangeKind::Untracked)
        .expect("untracked entry");
    assert_eq!(untracked.path, "文件.txt");
    assert!(!untracked.staged);
    assert!(!untracked.conflicted);
}

#[test]
fn porcelain_v2_rejects_non_utf8_paths_without_panicking() {
    let mut fixture = b"? ".to_vec();
    fixture.extend_from_slice(&[0xff, 0xfe]);
    fixture.push(0);
    assert!(matches!(
        parse_status_v2(&fixture),
        Err(AppError::InvalidInput(_))
    ));
}

#[test]
fn diff_summary_keeps_binary_marker_and_paths_with_double_dash() {
    let diff = b"diff --git a/--strange name.bin b/--strange name.bin\n"
        .iter()
        .chain(b"index 0000000..1111111\n".iter())
        .chain(b"Binary files a/--strange name.bin and b/--strange name.bin differ\n".iter())
        .copied()
        .collect::<Vec<_>>();
    let summary: DiffSummary = parse_diff_summary(&diff).expect("diff parses");
    assert_eq!(summary.files.len(), 1);
    assert!(summary.files[0].binary);
    assert_eq!(summary.files[0].path, "--strange name.bin");
}

#[test]
fn diff_summary_parses_unified_patch_paths_and_numstat() {
    let diff = b"diff --git a/old name.txt b/new name.txt\n--- a/old name.txt\n+++ b/new name.txt\n@@ -1 +1 @@\n-old\n+new\n1\t2\tnew name.txt\n";
    let summary = parse_diff_summary(diff).expect("unified diff parses");
    assert_eq!(summary.files.len(), 1);
    let file = &summary.files[0];
    assert_eq!(file.path, "new name.txt");
    assert_eq!(file.old_path.as_deref(), Some("old name.txt"));
    assert_eq!(file.new_path.as_deref(), Some("new name.txt"));
    assert_eq!((file.additions, file.deletions), (Some(1), Some(2)));
}

#[test]
fn diff_summary_parses_nul_numstat_binary_records() {
    let diff = b"1\t2\t--renamed file.txt\0-\t-\t--binary file.bin\0";
    let summary = parse_diff_summary(diff).expect("NUL numstat parses");
    assert_eq!(summary.files.len(), 2);
    assert_eq!(summary.files[0].path, "--renamed file.txt");
    assert_eq!(summary.files[1].path, "--binary file.bin");
    assert!(summary.files[1].binary);
}

#[test]
fn git_config_parser_returns_key_value_pairs() {
    let config = b"user.name\nAda Lovelace\0user.email\nada@example.test\0";
    let parsed = parse_git_config(config).expect("config parses");
    assert_eq!(
        parsed.get("user.name").map(String::as_str),
        Some("Ada Lovelace")
    );
    assert_eq!(
        parsed.get("user.email").map(String::as_str),
        Some("ada@example.test")
    );
    let empty = parse_git_config(b"core.editor\0\0").expect("empty config value parses");
    assert_eq!(empty.get("core.editor").map(String::as_str), Some(""));
}

#[test]
fn detect_repository_recognizes_normal_bare_and_worktree() {
    if Command::new("git").arg("--version").output().is_err() {
        eprintln!("git executable unavailable; skipping repository detection integration test");
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let normal = temp.path().join("normal");
    run_git(temp.path(), &["init", "--", normal.to_str().unwrap()]);
    run_git(&normal, &["config", "user.name", "Fixture User"]);
    run_git(&normal, &["config", "user.email", "fixture@example.test"]);
    fs::write(normal.join("seed.txt"), "seed\n").unwrap();
    run_git(&normal, &["add", "--", "seed.txt"]);
    run_git(&normal, &["commit", "--quiet", "-m", "seed"]);
    let bare = temp.path().join("bare.git");
    run_git(
        temp.path(),
        &["init", "--bare", "--", bare.to_str().unwrap()],
    );
    let worktree = temp.path().join("worktree");
    run_git(
        &normal,
        &[
            "worktree",
            "add",
            "--detach",
            "--",
            worktree.to_str().unwrap(),
        ],
    );

    let normal_info = detect_repository(&normal).unwrap();
    assert_eq!(normal_info.kind, RepositoryKind::Normal);
    assert_eq!(
        normal_info.canonical_path,
        fs::canonicalize(&normal).unwrap()
    );
    assert_eq!(
        normal_info.git_dir,
        fs::canonicalize(normal.join(".git")).unwrap()
    );
    let bare_info = detect_repository(&bare).unwrap();
    assert_eq!(bare_info.kind, RepositoryKind::Bare);
    assert_eq!(bare_info.canonical_path, fs::canonicalize(&bare).unwrap());
    assert_eq!(bare_info.git_dir, fs::canonicalize(&bare).unwrap());
    let worktree_info = detect_repository(&worktree).unwrap();
    assert_eq!(worktree_info.kind, RepositoryKind::Worktree);
    assert!(worktree_info.git_dir.is_absolute());
    assert!(matches!(
        detect_repository(temp.path().join("missing")),
        Err(AppError::InvalidInput(_))
    ));
    let ordinary = temp.path().join("ordinary");
    fs::create_dir(&ordinary).unwrap();
    assert!(matches!(
        detect_repository(&ordinary),
        Err(AppError::InvalidInput(_))
    ));
}

#[test]
fn detect_repository_rejects_fake_git_markers() {
    let temp = tempfile::tempdir().unwrap();
    let fake_dir = temp.path().join("fake-dir");
    fs::create_dir_all(fake_dir.join(".git")).unwrap();
    assert!(matches!(
        detect_repository(&fake_dir),
        Err(AppError::InvalidInput(_))
    ));

    let fake_file = temp.path().join("fake-file");
    fs::create_dir_all(&fake_file).unwrap();
    fs::write(fake_file.join(".git"), "gitdir: nowhere\n").unwrap();
    assert!(matches!(
        detect_repository(&fake_file),
        Err(AppError::InvalidInput(_))
    ));
}

#[test]
fn status_parser_rejects_interior_empty_nul_records() {
    let fixture = b"? first.txt\0\0? second.txt\0";
    assert!(matches!(
        parse_status_v2(fixture),
        Err(AppError::InvalidInput(_))
    ));
}

#[test]
fn status_parser_rejects_unknown_nonempty_records() {
    assert!(matches!(
        parse_status_v2(b"x unknown\0"),
        Err(AppError::InvalidInput(_))
    ));
}

#[test]
fn diff_parser_rejects_interior_empty_nul_records() {
    let fixture = b"1\t0\tfirst.txt\0\x00";
    let fixture = [fixture.as_slice(), b"1\t1\tsecond.txt\0"].concat();
    assert!(matches!(
        parse_diff_summary(fixture),
        Err(AppError::InvalidInput(_))
    ));
}

#[test]
fn diff_parser_keeps_name_status_nul_paths_after_double_dash() {
    let fixture = b"A\0--added file.txt\0";
    let summary = parse_diff_summary(fixture).expect("name-status NUL parses");
    assert_eq!(summary.files.len(), 1);
    assert_eq!(summary.files[0].path, "--added file.txt");
}

#[test]
fn real_repository_status_and_diff_round_trip_through_runner() {
    if Command::new("git").arg("--version").output().is_err() {
        eprintln!("git executable unavailable; skipping end-to-end integration test");
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    run_git(temp.path(), &["init", "--quiet"]);
    run_git(temp.path(), &["config", "user.name", "Fixture User"]);
    run_git(
        temp.path(),
        &["config", "user.email", "fixture@example.test"],
    );

    let tracked = "tracked file.txt";
    fs::write(temp.path().join(tracked), "before\n").unwrap();
    run_git(temp.path(), &["add", "--", tracked]);
    run_git(temp.path(), &["commit", "--quiet", "-m", "seed"]);
    fs::write(temp.path().join(tracked), "after\n").unwrap();
    let untracked = "未跟踪 file;echo PWNED.txt";
    fs::write(temp.path().join(untracked), "new\n").unwrap();

    let runner = SystemGitRunner::default();
    let status = runner
        .run(GitCommand {
            repo: temp.path().to_path_buf(),
            args: [
                "status",
                "--porcelain=v2",
                "-z",
                "--branch",
                "--untracked-files=all",
            ]
            .into_iter()
            .map(OsString::from)
            .collect(),
            stdin: None,
            timeout: Duration::from_secs(2),
        })
        .expect("status command runs");
    assert!(status.status.success());
    let snapshot = parse_status_v2(status.stdout).expect("status parses");
    assert_eq!(snapshot.changes.len(), 2);
    assert!(snapshot.changes.iter().any(|entry| entry.path == tracked));
    assert!(snapshot.changes.iter().any(|entry| entry.path == untracked));

    let diff = runner
        .run(GitCommand {
            repo: temp.path().to_path_buf(),
            args: [
                "diff",
                "--no-ext-diff",
                "--binary",
                "--numstat",
                "-z",
                "--",
                tracked,
            ]
            .into_iter()
            .map(OsString::from)
            .collect(),
            stdin: None,
            timeout: Duration::from_secs(2),
        })
        .expect("diff command runs");
    assert!(diff.status.success());
    let summary = parse_diff_summary(diff.stdout).expect("diff parses");
    assert_eq!(summary.files.len(), 1);
    assert_eq!(summary.files[0].path, tracked);

    let raw = runner
        .run(GitCommand {
            repo: temp.path().to_path_buf(),
            args: ["diff", "--raw", "-z", "--", tracked]
                .into_iter()
                .map(OsString::from)
                .collect(),
            stdin: None,
            timeout: Duration::from_secs(2),
        })
        .expect("raw diff command runs");
    assert!(raw.status.success());
    let raw_summary = parse_diff_summary(raw.stdout).expect("raw diff parses");
    assert_eq!(raw_summary.files.len(), 1);
    assert_eq!(raw_summary.files[0].path, tracked);

    let name_status = runner
        .run(GitCommand {
            repo: temp.path().to_path_buf(),
            args: ["diff", "--name-status", "-z", "--", tracked]
                .into_iter()
                .map(OsString::from)
                .collect(),
            stdin: None,
            timeout: Duration::from_secs(2),
        })
        .expect("name-status command runs");
    assert!(name_status.status.success());
    let name_summary = parse_diff_summary(name_status.stdout).expect("name-status parses");
    assert_eq!(name_summary.files.len(), 1);
    assert_eq!(name_summary.files[0].path, tracked);
}

#[test]
fn system_runner_treats_metacharacters_as_one_argument() {
    if Command::new("git").arg("--version").output().is_err() {
        eprintln!("git executable unavailable; skipping integration assertion");
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    run_git(temp.path(), &["init", "--quiet"]);
    let marker = temp.path().join("pwned");
    let tricky = format!("name;echo PWNED>{}", marker.display());
    let runner = SystemGitRunner::default();
    let output = runner
        .run(GitCommand {
            repo: temp.path().to_path_buf(),
            args: vec![
                OsString::from("status"),
                OsString::from("--porcelain=v2"),
                OsString::from("-z"),
                OsString::from("--"),
                OsString::from(&tricky),
            ],
            stdin: None,
            timeout: Duration::from_secs(2),
        })
        .expect("git status runs");
    assert!(output.status.success());
    assert!(!marker.exists(), "argument was interpreted by a shell");
}

#[test]
fn system_runner_enforces_output_and_timeout_bounds() {
    if Command::new("git").arg("--version").output().is_err() {
        eprintln!("git executable unavailable; skipping bound checks");
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    run_git(temp.path(), &["init", "--quiet"]);
    let limited = SystemGitRunner::with_output_limits(1, 1024);
    assert!(matches!(
        limited.run(GitCommand {
            repo: temp.path().to_path_buf(),
            args: vec![OsString::from("--version")],
            stdin: None,
            timeout: Duration::from_secs(2),
        }),
        Err(AppError::OutputLimit)
    ));

    let timed = SystemGitRunner::default();
    assert!(matches!(
        timed.run(GitCommand {
            repo: temp.path().to_path_buf(),
            args: vec![OsString::from("status")],
            stdin: None,
            timeout: Duration::from_nanos(1),
        }),
        Err(AppError::Timeout)
    ));
}

#[test]
fn git_errors_are_redacted_in_envelopes() {
    let error = AppError::Git("https://user:secret@example.test/repo failed".into());
    let debug = format!("{error:?}");
    assert!(!debug.contains("secret"));
    let envelope = ErrorEnvelope::from(error);
    assert_eq!(envelope.category, ErrorCategory::InternalFatal);
    assert!(!envelope.message.contains("secret"));
    assert!(!envelope.message.contains("user:"));
}

fn run_git(repo: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(repo)
        .args(args)
        .status()
        .expect("git executable is required for this test");
    assert!(status.success(), "git command failed: {args:?}");
}
