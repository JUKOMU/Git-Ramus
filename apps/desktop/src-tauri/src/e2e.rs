//! Debug-only native fixtures used by the WebDriver journey.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::commands::CommandResult;
use crate::error::{AppError, ErrorEnvelope};
use crate::git::engine::{DEFAULT_TIMEOUT, GitCommand, GitRunner, SystemGitRunner};
use crate::git::service::ProjectCreateInput;

pub const E2E_TEMP_PREFIX: &str = "git-ramus-e2e-";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct E2eFixture {
    pub root_path: String,
    pub projects: Vec<E2eProject>,
    pub primary_repository: E2eRepositoryReference,
    pub secondary_repository: E2eRepositoryReference,
    pub excluded_repository: E2eRepositoryReference,
    pub too_deep_repository: E2eRepositoryReference,
    pub changes: E2eChanges,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct E2eProject {
    pub project_id: String,
    pub root_path: String,
    pub name: String,
    pub scan_depth: i64,
    pub exclude_patterns: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct E2eRepositoryReference {
    pub display_name: String,
    pub relative_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct E2eChanges {
    pub staged_path: String,
    pub stage_path: String,
    pub remain_unstaged_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct E2eAppDataPaths {
    pub app_data_root: String,
    pub database_path: String,
}

#[derive(Debug)]
struct SeededFixtureFiles {
    root_path: PathBuf,
    primary_root: PathBuf,
    secondary_root: PathBuf,
    primary_repository: PathBuf,
    secondary_repository: PathBuf,
    excluded_repository: PathBuf,
    too_deep_repository: PathBuf,
}

#[tauri::command]
pub fn e2e_seed_fixture(state: State<'_, AppState>) -> CommandResult<E2eFixture> {
    seed_fixture(&state).map_err(|error| Box::new(ErrorEnvelope::from(error)))
}

#[tauri::command]
pub fn e2e_app_data_paths(state: State<'_, AppState>) -> CommandResult<E2eAppDataPaths> {
    (|| {
        Ok(E2eAppDataPaths {
            app_data_root: path_text(&state.e2e_app_data_root)?,
            database_path: path_text(&state.e2e_database_path)?,
        })
    })()
    .map_err(|error: AppError| Box::new(ErrorEnvelope::from(error)))
}

fn seed_fixture(state: &AppState) -> Result<E2eFixture, AppError> {
    let files = seed_fixture_files()?;
    let primary = match state.git.create_project(ProjectCreateInput {
        root_path: path_text(&files.primary_root)?,
        name: "E2E Primary".to_owned(),
        scan_depth: Some(3),
        exclude_patterns: vec!["excluded".to_owned()],
    }) {
        Ok(project) => project,
        Err(error) => {
            let _ = fs::remove_dir_all(&files.root_path);
            return Err(error);
        }
    };
    let secondary = match state.git.create_project(ProjectCreateInput {
        root_path: path_text(&files.secondary_root)?,
        name: "E2E Secondary".to_owned(),
        scan_depth: Some(1),
        exclude_patterns: Vec::new(),
    }) {
        Ok(project) => project,
        Err(error) => {
            let _ = state.git.delete_project_by_id(&primary.id);
            let _ = fs::remove_dir_all(&files.root_path);
            return Err(error);
        }
    };

    Ok(E2eFixture {
        root_path: path_text(&files.root_path)?,
        projects: vec![
            E2eProject {
                project_id: primary.id,
                root_path: primary.root_path,
                name: primary.name,
                scan_depth: primary.scan_depth,
                exclude_patterns: primary.exclude_patterns,
            },
            E2eProject {
                project_id: secondary.id,
                root_path: secondary.root_path,
                name: secondary.name,
                scan_depth: secondary.scan_depth,
                exclude_patterns: secondary.exclude_patterns,
            },
        ],
        primary_repository: repository_reference(&files.primary_root, &files.primary_repository)?,
        secondary_repository: repository_reference(
            &files.secondary_root,
            &files.secondary_repository,
        )?,
        excluded_repository: repository_reference(&files.primary_root, &files.excluded_repository)?,
        too_deep_repository: repository_reference(&files.primary_root, &files.too_deep_repository)?,
        changes: E2eChanges {
            staged_path: "staged.txt".to_owned(),
            stage_path: "unstaged.txt".to_owned(),
            remain_unstaged_path: "untracked.txt".to_owned(),
        },
    })
}

fn seed_fixture_files() -> Result<SeededFixtureFiles, AppError> {
    let root_path = create_guarded_temp_root()?;
    let result = (|| {
        let git_home = root_path.join("git-home");
        let xdg_config_home = root_path.join("git-xdg");
        let global_config = root_path.join("gitconfig");
        fs::create_dir(&git_home)?;
        fs::create_dir(&xdg_config_home)?;
        fs::write(&global_config, b"")?;
        let runner =
            SystemGitRunner::new().with_sealed_config(git_home, xdg_config_home, global_config);
        let primary_root = root_path.join("project-primary");
        let secondary_root = root_path.join("project-secondary");
        let primary_repository = primary_root.join("level-one/level-two/included-repository");
        let secondary_repository = secondary_root.join("secondary-repository");
        let excluded_repository = primary_root.join("excluded/excluded-repository");
        let too_deep_repository = primary_root.join("level-one/level-two/level-three/too-deep");

        create_repository(&runner, &primary_repository, true)?;
        create_repository(&runner, &secondary_repository, false)?;
        create_repository(&runner, &excluded_repository, false)?;
        create_repository(&runner, &too_deep_repository, false)?;

        Ok(SeededFixtureFiles {
            root_path: root_path.clone(),
            primary_root,
            secondary_root,
            primary_repository,
            secondary_repository,
            excluded_repository,
            too_deep_repository,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&root_path);
    }
    result
}

fn create_guarded_temp_root() -> Result<PathBuf, AppError> {
    let temp = std::env::temp_dir();
    for _ in 0..4 {
        let candidate = temp.join(format!("{E2E_TEMP_PREFIX}{}", Uuid::new_v4()));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(AppError::InvalidInput(
        "unable to allocate an E2E fixture directory".to_owned(),
    ))
}

fn create_repository(
    runner: &SystemGitRunner,
    repository: &Path,
    dirty: bool,
) -> Result<(), AppError> {
    fs::create_dir_all(repository)?;
    run_git(
        runner,
        repository,
        ["-c", "init.defaultBranch=main", "init"],
    )?;
    fs::write(repository.join("initial.txt"), b"initial\n")?;
    fs::write(repository.join("staged.txt"), b"staged initial\n")?;
    fs::write(repository.join("unstaged.txt"), b"unstaged initial\n")?;
    run_git(
        runner,
        repository,
        ["add", "--", "initial.txt", "staged.txt", "unstaged.txt"],
    )?;
    run_git(
        runner,
        repository,
        [
            "-c",
            "user.name=Git-Ramus E2E",
            "-c",
            "user.email=e2e@example.invalid",
            "-c",
            "commit.gpgSign=false",
            "commit",
            "-m",
            "E2E fixture baseline",
        ],
    )?;
    if dirty {
        fs::write(repository.join("staged.txt"), b"staged changed\n")?;
        fs::write(repository.join("unstaged.txt"), b"unstaged changed\n")?;
        fs::write(repository.join("untracked.txt"), b"untracked\n")?;
        run_git(runner, repository, ["add", "--", "staged.txt"])?;
    }
    Ok(())
}

fn run_git<const N: usize>(
    runner: &SystemGitRunner,
    repository: &Path,
    args: [&str; N],
) -> Result<(), AppError> {
    let output = runner.run(GitCommand {
        repo: repository.to_path_buf(),
        args: args.into_iter().map(OsString::from).collect(),
        stdin: None,
        timeout: DEFAULT_TIMEOUT,
    })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(AppError::Git("E2E fixture Git command failed".to_owned()))
    }
}

fn repository_reference(
    project_root: &Path,
    repository: &Path,
) -> Result<E2eRepositoryReference, AppError> {
    let display_name = repository
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(AppError::NonUtf8Path)?
        .to_owned();
    let relative = repository
        .strip_prefix(project_root)
        .map_err(|_| AppError::InvalidInput("fixture repository escaped its project".to_owned()))?;
    Ok(E2eRepositoryReference {
        display_name,
        relative_path: relative.to_string_lossy().replace('\\', "/"),
    })
}

fn path_text(path: &Path) -> Result<String, AppError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or(AppError::NonUtf8Path)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use super::{E2E_TEMP_PREFIX, seed_fixture_files};
    use crate::git::engine::{DEFAULT_TIMEOUT, GitCommand, GitRunner, SystemGitRunner};

    const CONFIG_ATTACK_CHILD: &str = "GIT_RAMUS_E2E_CONFIG_ATTACK_CHILD";
    const CONFIG_ATTACK_MARKER: &str = "GIT_RAMUS_E2E_CONFIG_ATTACK_MARKER";

    #[test]
    fn fixture_ignores_malicious_global_git_configuration() {
        let attack_root =
            std::env::temp_dir().join(format!("git-ramus-config-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&attack_root).expect("attack root creates");
        let marker = attack_root.join("global-hook-ran");
        let template_marker = attack_root.join("template-hook-ran");
        install_malicious_git_configuration(&attack_root, &marker, &template_marker);

        let output = Command::new(std::env::current_exe().expect("test executable resolves"))
            .args([
                "--exact",
                "e2e::tests::fixture_ignores_malicious_global_git_configuration_child",
                "--nocapture",
            ])
            .env(CONFIG_ATTACK_CHILD, "1")
            .env(CONFIG_ATTACK_MARKER, &marker)
            .env("HOME", &attack_root)
            .env("USERPROFILE", &attack_root)
            .env("XDG_CONFIG_HOME", attack_root.join("xdg"))
            .output()
            .expect("isolated child test runs");

        let _ = fs::remove_dir_all(&attack_root);
        assert!(
            output.status.success(),
            "fixture inherited malicious Git configuration:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn fixture_ignores_malicious_global_git_configuration_child() {
        if std::env::var_os(CONFIG_ATTACK_CHILD).is_none() {
            return;
        }
        let marker = PathBuf::from(
            std::env::var_os(CONFIG_ATTACK_MARKER).expect("attack marker is configured"),
        );
        let fixture = seed_fixture_files().expect("fixture seeds despite hostile config");
        fs::remove_dir_all(&fixture.root_path).expect("fixture cleans up");
        assert!(!marker.exists(), "global hooksPath executed");
        assert!(
            !marker.with_file_name("template-hook-ran").exists(),
            "global init.templateDir executed"
        );
    }

    fn install_malicious_git_configuration(root: &Path, marker: &Path, template_marker: &Path) {
        let hooks = root.join("hooks");
        let template_hooks = root.join("template/hooks");
        fs::create_dir_all(&hooks).expect("global hooks create");
        fs::create_dir_all(&template_hooks).expect("template hooks create");
        write_hook(&hooks.join("post-commit"), marker);
        write_hook(&template_hooks.join("post-commit"), template_marker);
        let included = root.join("included.gitconfig");
        fs::write(
            &included,
            format!("[core]\n\thooksPath = {}\n", git_path(&hooks)),
        )
        .expect("included config writes");
        fs::write(
            root.join(".gitconfig"),
            format!(
                "[include]\n\tpath = {}\n[init]\n\ttemplateDir = {}\n",
                git_path(&included),
                git_path(&root.join("template"))
            ),
        )
        .expect("global config writes");
    }

    fn write_hook(path: &Path, marker: &Path) {
        fs::write(
            path,
            format!("#!/bin/sh\nprintf compromised > \"{}\"\n", git_path(marker)),
        )
        .expect("hook writes");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o755))
                .expect("hook becomes executable");
        }
    }

    fn git_path(path: &Path) -> String {
        path.to_string_lossy().replace('\\', "/")
    }

    #[test]
    fn fixture_uses_a_guarded_temp_root_and_real_isolated_git_repositories() {
        let fixture = seed_fixture_files().expect("fixture seeds");
        let root_name = fixture
            .root_path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("fixture root name is UTF-8");
        assert!(root_name.starts_with(E2E_TEMP_PREFIX));
        assert!(fixture.primary_repository.join(".git").is_dir());
        assert!(fixture.secondary_repository.join(".git").is_dir());
        assert!(fixture.excluded_repository.join(".git").is_dir());
        assert!(fixture.too_deep_repository.join(".git").is_dir());

        let status = SystemGitRunner::new()
            .run(GitCommand {
                repo: fixture.primary_repository.clone(),
                args: ["status", "--porcelain=v1", "--untracked-files=all"]
                    .map(Into::into)
                    .to_vec(),
                stdin: None,
                timeout: DEFAULT_TIMEOUT,
            })
            .expect("fixture status reads");
        let status = String::from_utf8(status.stdout).expect("status is UTF-8");
        assert!(status.contains("M  staged.txt"));
        assert!(status.contains(" M unstaged.txt"));
        assert!(status.contains("?? untracked.txt"));

        fs::remove_dir_all(&fixture.root_path).expect("fixture cleans up");
    }
}
