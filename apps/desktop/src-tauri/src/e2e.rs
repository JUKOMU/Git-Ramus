//! Debug-only native fixtures used by the WebDriver journey.

use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::commands::CommandResult;
use crate::error::{AppError, ErrorEnvelope};
use crate::git::engine::{DEFAULT_TIMEOUT, GitCommand, GitRunner, SystemGitRunner};
use crate::git::service::ProjectCreateInput;
use crate::providers::e2e_adapter::{
    E2E_PROVIDER_BASE_URL, E2E_PROVIDER_SSH_URL, E2E_PROVIDER_TOKEN,
};
use crate::providers::model::{
    ProviderAccountSummary, ProviderArchivedFilter, ProviderInstanceSummary, ProviderKind,
    ProviderRepositoryDirection, ProviderRepositoryQuery, ProviderRepositorySort, RemoteRepository,
};
use crate::providers::service::{CreateInstanceInput, ListRepositoriesInput};
use crate::secrets::SensitiveString;

pub const E2E_TEMP_PREFIX: &str = "git-ramus-e2e-";
pub const E2E_TRANSPORT_TEMP_PREFIX: &str = "git-ramus-e2e-transport-";
pub const E2E_TRANSPORT_PUBLIC_URL: &str = "https://gitlab.example.test/skills/private-skill.git";
const E2E_TRANSPORT_PROJECT_NAME: &str = "E2E Transport";
const E2E_TRANSPORT_REPOSITORY_NAME: &str = "private-skill";
const E2E_TRANSPORT_BRANCH: &str = "main";

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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct E2eProviderFixture {
    pub instance: ProviderInstanceSummary,
    pub account: ProviderAccountSummary,
    pub repository: RemoteRepository,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct E2eTransportFixture {
    pub project_id: String,
    pub project_name: String,
    pub repository_name: String,
    pub branch_name: String,
    pub remote_name: String,
    pub cleanup_token: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct E2eTransportTokenRequest {
    pub cleanup_token: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct E2eTransportRepositoryRequest {
    pub cleanup_token: String,
    pub project_id: String,
    pub repository_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct E2eTransportHead {
    pub head_oid: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct E2eTransportBlockStatus {
    pub connected: bool,
    pub active: bool,
}

#[derive(Debug, Clone)]
struct SeededTransportFixture {
    root_path: PathBuf,
    destination_parent: PathBuf,
    bare_remote: PathBuf,
    remote_writer: PathBuf,
    git_home: PathBuf,
    xdg_config_home: PathBuf,
    sealed_global_config: PathBuf,
    public_url: &'static str,
}

#[derive(Debug, Clone)]
struct RegisteredTransportFixture {
    files: SeededTransportFixture,
    project_id: String,
    cleanup_token: String,
    destination_pending: bool,
    remote_sequence: u32,
    blocked_global_config: Option<PathBuf>,
    block_next_fetch: bool,
    blocking_server: Option<Arc<BlockingHttpServer>>,
}

#[derive(Debug)]
struct BlockingHttpServer {
    base_url: String,
    connected: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
    shutdown: mpsc::Sender<()>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

#[derive(Debug, Clone)]
pub(crate) struct E2eTransportGitConfig {
    pub home: PathBuf,
    pub xdg_config_home: PathBuf,
    pub global_config: PathBuf,
}

#[derive(Clone, Default)]
pub(crate) struct E2eTransportRegistry {
    fixture: Arc<Mutex<Option<RegisteredTransportFixture>>>,
}

impl BlockingHttpServer {
    fn start() -> Result<Self, AppError> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let (shutdown, shutdown_rx) = mpsc::channel();
        let connected = Arc::new(AtomicBool::new(false));
        let connected_for_thread = connected.clone();
        let active = Arc::new(AtomicBool::new(false));
        let active_for_thread = active.clone();
        let thread = thread::spawn(move || {
            let mut held_stream = None;
            loop {
                if shutdown_rx.try_recv().is_ok() {
                    break;
                }
                if held_stream.is_none() {
                    match listener.accept() {
                        Ok((stream, _)) if stream.set_nonblocking(true).is_ok() => {
                            // Publish active first so re-arm can never observe a false terminal
                            // state between the two atomics.
                            active_for_thread.store(true, Ordering::Release);
                            connected_for_thread.store(true, Ordering::Release);
                            held_stream = Some(stream);
                        }
                        Ok(_) => break,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                        Err(_) => break,
                    }
                }
                let connection_closed = held_stream.as_mut().is_some_and(|stream| {
                    let mut buffer = [0_u8; 1024];
                    match stream.read(&mut buffer) {
                        Ok(0) => true,
                        Ok(_) => false,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => false,
                        Err(_) => true,
                    }
                });
                if connection_closed {
                    active_for_thread.store(false, Ordering::Release);
                    held_stream = None;
                }
                thread::sleep(Duration::from_millis(10));
            }
            drop(held_stream);
            active_for_thread.store(false, Ordering::Release);
        });
        Ok(Self {
            base_url: format!("http://{address}/skills/private-skill.git"),
            connected,
            active,
            shutdown,
            thread: Mutex::new(Some(thread)),
        })
    }
}

impl Drop for BlockingHttpServer {
    fn drop(&mut self) {
        let _ = self.shutdown.send(());
        if let Some(thread) = self.thread.lock().take() {
            let _ = thread.join();
        }
    }
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

#[tauri::command]
pub async fn e2e_seed_provider_fixture(
    state: State<'_, AppState>,
) -> CommandResult<E2eProviderFixture> {
    seed_provider_fixture(&state)
        .await
        .map_err(|error| Box::new(ErrorEnvelope::from(error)))
}

#[tauri::command]
pub fn e2e_seed_transport_fixture(
    state: State<'_, AppState>,
) -> CommandResult<E2eTransportFixture> {
    (|| {
        let files = seed_transport_fixture()?;
        let project = match state.git.create_project(ProjectCreateInput {
            root_path: path_text(&files.destination_parent)?,
            name: E2E_TRANSPORT_PROJECT_NAME.to_owned(),
            scan_depth: Some(1),
            exclude_patterns: Vec::new(),
        }) {
            Ok(project) => project,
            Err(error) => {
                let _ = cleanup_transport_fixture(&files);
                return Err(error);
            }
        };
        let cleanup_token = Uuid::new_v4().to_string();
        if let Err(error) =
            state
                .e2e_transport
                .install(files, project.id.clone(), cleanup_token.clone())
        {
            let _ = state.git.delete_project_by_id(&project.id);
            return Err(error);
        }
        Ok(E2eTransportFixture {
            project_id: project.id,
            project_name: project.name,
            repository_name: E2E_TRANSPORT_REPOSITORY_NAME.to_owned(),
            branch_name: E2E_TRANSPORT_BRANCH.to_owned(),
            remote_name: "origin".to_owned(),
            cleanup_token,
        })
    })()
    .map_err(|error: AppError| Box::new(ErrorEnvelope::from(error)))
}

#[tauri::command]
pub fn e2e_advance_transport_remote(
    state: State<'_, AppState>,
    request: E2eTransportTokenRequest,
) -> CommandResult<E2eTransportHead> {
    state
        .e2e_transport
        .advance_remote(&request.cleanup_token)
        .map(|head_oid| E2eTransportHead { head_oid })
        .map_err(|error| Box::new(ErrorEnvelope::from(error)))
}

#[tauri::command]
pub fn e2e_commit_transport_local(
    state: State<'_, AppState>,
    request: E2eTransportRepositoryRequest,
) -> CommandResult<E2eTransportHead> {
    (|| {
        let repository = state
            .git
            .repository_repository()
            .get(&request.repository_id)?;
        if !state
            .git
            .repository_repository()
            .is_in_project(&request.project_id, &request.repository_id)?
        {
            return Err(AppError::PermissionDenied);
        }
        let head_oid = state.e2e_transport.commit_local(
            &request.cleanup_token,
            &request.project_id,
            Path::new(&repository.canonical_path),
        )?;
        state.git.refresh_repository_in_context(
            &crate::git::service::QueryContext::project(request.project_id),
            &request.repository_id,
        )?;
        Ok(E2eTransportHead { head_oid })
    })()
    .map_err(|error: AppError| Box::new(ErrorEnvelope::from(error)))
}

#[tauri::command]
pub fn e2e_transport_remote_head(
    state: State<'_, AppState>,
    request: E2eTransportTokenRequest,
) -> CommandResult<E2eTransportHead> {
    state
        .e2e_transport
        .remote_head(&request.cleanup_token)
        .map(|head_oid| E2eTransportHead { head_oid })
        .map_err(|error| Box::new(ErrorEnvelope::from(error)))
}

#[tauri::command]
pub fn e2e_block_transport_fetch(
    state: State<'_, AppState>,
    request: E2eTransportTokenRequest,
) -> CommandResult<()> {
    state
        .e2e_transport
        .block_next_fetch(&request.cleanup_token)
        .map_err(|error| Box::new(ErrorEnvelope::from(error)))
}

#[tauri::command]
pub fn e2e_transport_block_status(
    state: State<'_, AppState>,
    request: E2eTransportTokenRequest,
) -> CommandResult<E2eTransportBlockStatus> {
    state
        .e2e_transport
        .blocked_fetch_status(&request.cleanup_token)
        .map(|(connected, active)| E2eTransportBlockStatus { connected, active })
        .map_err(|error| Box::new(ErrorEnvelope::from(error)))
}

#[tauri::command]
pub fn e2e_cleanup_transport_fixture(
    state: State<'_, AppState>,
    request: E2eTransportTokenRequest,
) -> CommandResult<()> {
    state
        .e2e_transport
        .cleanup(&request.cleanup_token)
        .map_err(|error| Box::new(ErrorEnvelope::from(error)))
}

async fn seed_provider_fixture(state: &AppState) -> Result<E2eProviderFixture, AppError> {
    let instance = match state
        .providers
        .list_instances()?
        .into_iter()
        .find(|candidate| {
            candidate.provider_kind == ProviderKind::Gitlab
                && candidate.base_url == E2E_PROVIDER_BASE_URL
        }) {
        Some(existing) => state.providers.validate_instance(&existing.id).await?,
        None => {
            state
                .providers
                .create_instance(CreateInstanceInput {
                    provider_kind: ProviderKind::Gitlab,
                    display_name: "E2E GitLab".to_owned(),
                    base_url: E2E_PROVIDER_BASE_URL.to_owned(),
                    custom_ca_path: None,
                })
                .await?
        }
    };
    let account = match state
        .providers
        .list_accounts(&instance.id)?
        .into_iter()
        .find(|candidate| candidate.provider_user_id == "9001")
    {
        Some(existing) => state.providers.validate_account(&existing.id).await?,
        None => {
            state
                .providers
                .connect_account(
                    &instance.id,
                    SensitiveString::new(E2E_PROVIDER_TOKEN.to_owned()),
                )
                .await?
        }
    };
    let page = state
        .providers
        .list_repositories(
            "git-ramus.provider-center",
            ListRepositoriesInput {
                account_id: account.id.clone(),
                query: ProviderRepositoryQuery {
                    search: String::new(),
                    visibility: None,
                    namespace: None,
                    archived: ProviderArchivedFilter::All,
                    sort: ProviderRepositorySort::Name,
                    direction: ProviderRepositoryDirection::Asc,
                    page_size: 30,
                },
                cursor: None,
                operation_id: Uuid::new_v4().to_string(),
            },
        )
        .await?;
    let repository = page
        .items
        .into_iter()
        .find(|candidate| candidate.repository_id == "4242")
        .ok_or_else(|| AppError::NotFound("E2E Provider repository".to_owned()))?;
    Ok(E2eProviderFixture {
        instance,
        account,
        repository,
    })
}

impl E2eTransportRegistry {
    fn install(
        &self,
        files: SeededTransportFixture,
        project_id: String,
        cleanup_token: String,
    ) -> Result<(), AppError> {
        validate_transport_token(&cleanup_token)?;
        if files.public_url != E2E_TRANSPORT_PUBLIC_URL {
            cleanup_transport_fixture(&files)?;
            return Err(AppError::PermissionDenied);
        }
        let mut fixture = self.fixture.lock();
        if fixture.is_some() {
            drop(fixture);
            cleanup_transport_fixture(&files)?;
            return Err(AppError::InvalidInput(
                "an E2E Transport fixture is already active".to_owned(),
            ));
        }
        *fixture = Some(RegisteredTransportFixture {
            files,
            project_id,
            cleanup_token,
            destination_pending: true,
            remote_sequence: 0,
            blocked_global_config: None,
            block_next_fetch: false,
            blocking_server: None,
        });
        Ok(())
    }

    pub(crate) fn execution_config(
        &self,
        command: &GitCommand,
    ) -> Result<Option<E2eTransportGitConfig>, AppError> {
        let mut fixture = self.fixture.lock();
        let Some(fixture) = fixture.as_mut() else {
            return Ok(None);
        };
        validate_transport_root(&fixture.files.root_path)?;
        let canonical_repository = fs::canonicalize(&command.repo)?;
        let canonical_destination = fs::canonicalize(&fixture.files.destination_parent)?;
        if canonical_repository != canonical_destination
            && !canonical_repository.starts_with(&canonical_destination)
        {
            return Ok(None);
        }
        for path in [
            &fixture.files.git_home,
            &fixture.files.xdg_config_home,
            &fixture.files.sealed_global_config,
        ] {
            if !path.starts_with(&fixture.files.root_path) {
                return Err(AppError::PermissionDenied);
            }
        }
        let global_config =
            if fixture.block_next_fetch && git_command_name(&command.args) == "fetch" {
                fixture.block_next_fetch = false;
                fixture.blocked_global_config.clone().ok_or_else(|| {
                    AppError::InvalidInput("blocked Fetch is not armed".to_owned())
                })?
            } else {
                fixture.files.sealed_global_config.clone()
            };
        if !global_config.starts_with(&fixture.files.root_path) || !global_config.is_file() {
            return Err(AppError::PermissionDenied);
        }
        Ok(Some(E2eTransportGitConfig {
            home: fixture.files.git_home.clone(),
            xdg_config_home: fixture.files.xdg_config_home.clone(),
            global_config,
        }))
    }

    pub(crate) fn take_destination_parent(&self) -> Result<Option<String>, AppError> {
        let mut fixture = self.fixture.lock();
        let Some(fixture) = fixture.as_mut() else {
            return Ok(None);
        };
        if !fixture.destination_pending {
            return Ok(None);
        }
        validate_transport_root(&fixture.files.root_path)?;
        let canonical_destination = fs::canonicalize(&fixture.files.destination_parent)?;
        let canonical_root = fs::canonicalize(&fixture.files.root_path)?;
        if canonical_destination.parent() != Some(canonical_root.as_path()) {
            return Err(AppError::PermissionDenied);
        }
        fixture.destination_pending = false;
        path_text(&fixture.files.destination_parent).map(Some)
    }

    fn registered(&self, cleanup_token: &str) -> Result<RegisteredTransportFixture, AppError> {
        validate_transport_token(cleanup_token)?;
        let fixture = self.fixture.lock();
        let fixture = fixture
            .as_ref()
            .ok_or_else(|| AppError::NotFound("E2E Transport fixture".to_owned()))?;
        if fixture.cleanup_token != cleanup_token {
            return Err(AppError::PermissionDenied);
        }
        validate_transport_root(&fixture.files.root_path)?;
        Ok(fixture.clone())
    }

    fn block_next_fetch(&self, cleanup_token: &str) -> Result<(), AppError> {
        validate_transport_token(cleanup_token)?;
        let mut fixture = self.fixture.lock();
        let fixture = fixture
            .as_mut()
            .ok_or_else(|| AppError::NotFound("E2E Transport fixture".to_owned()))?;
        if fixture.cleanup_token != cleanup_token {
            return Err(AppError::PermissionDenied);
        }
        let blocked_generation_in_flight = fixture.blocking_server.as_ref().is_some_and(|server| {
            !server.connected.load(Ordering::Acquire) || server.active.load(Ordering::Acquire)
        });
        if fixture.block_next_fetch || blocked_generation_in_flight {
            return Err(AppError::InvalidInput(
                "a blocked Fetch is already armed".to_owned(),
            ));
        }
        validate_transport_root(&fixture.files.root_path)?;
        let server = Arc::new(BlockingHttpServer::start()?);
        let blocked_global_config = fixture.files.root_path.join("blocked-transport.gitconfig");
        fs::write(
            &blocked_global_config,
            format!(
                "[url \"{}\"]\n\tinsteadOf = {E2E_TRANSPORT_PUBLIC_URL}\n",
                server.base_url
            ),
        )?;
        fixture.blocked_global_config = Some(blocked_global_config);
        fixture.blocking_server = Some(server);
        fixture.block_next_fetch = true;
        Ok(())
    }

    fn blocked_fetch_status(&self, cleanup_token: &str) -> Result<(bool, bool), AppError> {
        let fixture = self.registered(cleanup_token)?;
        Ok(fixture
            .blocking_server
            .as_ref()
            .map_or((false, false), |server| {
                (
                    server.connected.load(Ordering::Acquire),
                    server.active.load(Ordering::Acquire),
                )
            }))
    }

    fn advance_remote(&self, cleanup_token: &str) -> Result<String, AppError> {
        validate_transport_token(cleanup_token)?;
        let (fixture, sequence) = {
            let mut registered = self.fixture.lock();
            let registered = registered
                .as_mut()
                .ok_or_else(|| AppError::NotFound("E2E Transport fixture".to_owned()))?;
            if registered.cleanup_token != cleanup_token {
                return Err(AppError::PermissionDenied);
            }
            validate_transport_root(&registered.files.root_path)?;
            registered.remote_sequence = registered
                .remote_sequence
                .checked_add(1)
                .ok_or(AppError::OutputLimit)?;
            (registered.files.clone(), registered.remote_sequence)
        };
        let file_name = format!("remote-update-{sequence}.txt");
        fs::write(
            fixture.remote_writer.join(&file_name),
            format!("remote update {sequence}\n"),
        )?;
        let runner = transport_fixture_runner(&fixture);
        run_git_dynamic(
            &runner,
            &fixture.remote_writer,
            vec![
                OsString::from("add"),
                OsString::from("--"),
                file_name.into(),
            ],
        )?;
        run_git(
            &runner,
            &fixture.remote_writer,
            [
                "-c",
                "user.name=Git-Ramus E2E",
                "-c",
                "user.email=e2e@example.invalid",
                "-c",
                "commit.gpgSign=false",
                "commit",
                "-m",
                "E2E remote update",
            ],
        )?;
        run_git(
            &runner,
            &fixture.remote_writer,
            ["push", "--quiet", "origin", E2E_TRANSPORT_BRANCH],
        )?;
        git_stdout(
            &runner,
            &fixture.remote_writer,
            ["rev-parse", E2E_TRANSPORT_BRANCH],
        )
    }

    fn commit_local(
        &self,
        cleanup_token: &str,
        project_id: &str,
        repository: &Path,
    ) -> Result<String, AppError> {
        let registered = self.registered(cleanup_token)?;
        if registered.project_id != project_id {
            return Err(AppError::PermissionDenied);
        }
        let repository = fs::canonicalize(repository)?;
        let destination = fs::canonicalize(&registered.files.destination_parent)?;
        if repository.parent() != Some(destination.as_path()) {
            return Err(AppError::PermissionDenied);
        }
        fs::write(repository.join("local-update.txt"), b"local update\n")?;
        let runner = transport_fixture_runner(&registered.files);
        run_git(&runner, &repository, ["add", "--", "local-update.txt"])?;
        run_git(
            &runner,
            &repository,
            [
                "-c",
                "user.name=Git-Ramus E2E",
                "-c",
                "user.email=e2e@example.invalid",
                "-c",
                "commit.gpgSign=false",
                "commit",
                "-m",
                "E2E local update",
            ],
        )?;
        git_stdout(&runner, &repository, ["rev-parse", "HEAD"])
    }

    fn remote_head(&self, cleanup_token: &str) -> Result<String, AppError> {
        let registered = self.registered(cleanup_token)?;
        let runner = transport_fixture_runner(&registered.files);
        git_stdout(
            &runner,
            &registered.files.bare_remote,
            ["rev-parse", "refs/heads/main"],
        )
    }

    fn cleanup(&self, cleanup_token: &str) -> Result<(), AppError> {
        validate_transport_token(cleanup_token)?;
        let registered = {
            let mut fixture = self.fixture.lock();
            let registered = fixture
                .as_ref()
                .ok_or_else(|| AppError::NotFound("E2E Transport fixture".to_owned()))?;
            if registered.cleanup_token != cleanup_token {
                return Err(AppError::PermissionDenied);
            }
            fixture
                .take()
                .expect("checked E2E Transport fixture remains registered")
        };
        match cleanup_transport_fixture(&registered.files) {
            Ok(()) => Ok(()),
            Err(error) => {
                *self.fixture.lock() = Some(registered);
                Err(error)
            }
        }
    }
}

fn seed_transport_fixture() -> Result<SeededTransportFixture, AppError> {
    let root_path = create_guarded_transport_temp_root()?;
    let result = (|| {
        let git_home = root_path.join("git-home");
        let xdg_config_home = root_path.join("git-xdg");
        let sealed_global_config = root_path.join("transport.gitconfig");
        let bare_remote = root_path.join("remote.git");
        let remote_writer = root_path.join("remote-writer");
        let destination_parent = root_path.join("destinations");
        for directory in [
            &git_home,
            &xdg_config_home,
            &bare_remote,
            &remote_writer,
            &destination_parent,
        ] {
            fs::create_dir(directory)?;
        }
        fs::write(&sealed_global_config, b"")?;
        let runner = SystemGitRunner::new().with_sealed_config(
            git_home.clone(),
            xdg_config_home.clone(),
            sealed_global_config.clone(),
        );
        run_git(&runner, &bare_remote, ["init", "--bare", "--quiet"])?;
        run_git(
            &runner,
            &remote_writer,
            ["-c", "init.defaultBranch=main", "init", "--quiet"],
        )?;
        fs::write(remote_writer.join("seed.txt"), b"seed\n")?;
        run_git(&runner, &remote_writer, ["add", "--", "seed.txt"])?;
        run_git(
            &runner,
            &remote_writer,
            [
                "-c",
                "user.name=Git-Ramus E2E",
                "-c",
                "user.email=e2e@example.invalid",
                "-c",
                "commit.gpgSign=false",
                "commit",
                "--quiet",
                "-m",
                "E2E transport seed",
            ],
        )?;
        run_git(
            &runner,
            &remote_writer,
            ["branch", "-M", E2E_TRANSPORT_BRANCH],
        )?;
        let bare_url = url::Url::from_file_path(&bare_remote)
            .map_err(|()| AppError::InvalidInput("Bare Remote path is not a file URL".to_owned()))?
            .to_string();
        run_git_dynamic(
            &runner,
            &remote_writer,
            vec![
                OsString::from("remote"),
                OsString::from("add"),
                OsString::from("origin"),
                OsString::from(&bare_url),
            ],
        )?;
        run_git(
            &runner,
            &remote_writer,
            ["push", "--quiet", "-u", "origin", E2E_TRANSPORT_BRANCH],
        )?;
        run_git(
            &runner,
            &bare_remote,
            ["symbolic-ref", "HEAD", "refs/heads/main"],
        )?;
        fs::write(
            &sealed_global_config,
            format!(
                "[url \"{bare_url}\"]\n\tinsteadOf = {E2E_TRANSPORT_PUBLIC_URL}\n[protocol \"file\"]\n\tallow = always\n"
            ),
        )?;
        Ok(SeededTransportFixture {
            root_path: root_path.clone(),
            destination_parent,
            bare_remote,
            remote_writer,
            git_home,
            xdg_config_home,
            sealed_global_config,
            public_url: E2E_TRANSPORT_PUBLIC_URL,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&root_path);
    }
    result
}

fn cleanup_transport_fixture(fixture: &SeededTransportFixture) -> Result<(), AppError> {
    let root = validate_transport_root(&fixture.root_path)?;
    fs::remove_dir_all(root)?;
    Ok(())
}

fn create_guarded_transport_temp_root() -> Result<PathBuf, AppError> {
    let temp = std::env::temp_dir();
    for _ in 0..4 {
        let candidate = temp.join(format!("{E2E_TRANSPORT_TEMP_PREFIX}{}", Uuid::new_v4()));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(AppError::InvalidInput(
        "unable to allocate an E2E Transport fixture directory".to_owned(),
    ))
}

fn validate_transport_root(root: &Path) -> Result<PathBuf, AppError> {
    let metadata = fs::symlink_metadata(root)?;
    if !metadata.is_dir() || is_symlink_or_reparse_point(&metadata) {
        return Err(AppError::PermissionDenied);
    }
    let root = fs::canonicalize(root)?;
    let temp = fs::canonicalize(std::env::temp_dir())?;
    let safe_name = root
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(E2E_TRANSPORT_TEMP_PREFIX));
    if root.parent() != Some(temp.as_path()) || !safe_name {
        return Err(AppError::PermissionDenied);
    }
    Ok(root)
}

fn validate_transport_token(value: &str) -> Result<(), AppError> {
    let parsed = Uuid::parse_str(value)
        .map_err(|_| AppError::InvalidInput("E2E cleanup token must be a UUID".to_owned()))?;
    if parsed.to_string() != value {
        return Err(AppError::InvalidInput(
            "E2E cleanup token must be canonical".to_owned(),
        ));
    }
    Ok(())
}

fn transport_fixture_runner(fixture: &SeededTransportFixture) -> SystemGitRunner {
    SystemGitRunner::new().with_sealed_config(
        fixture.git_home.clone(),
        fixture.xdg_config_home.clone(),
        fixture.sealed_global_config.clone(),
    )
}

fn run_git_dynamic(
    runner: &SystemGitRunner,
    repository: &Path,
    args: Vec<OsString>,
) -> Result<(), AppError> {
    #[cfg(test)]
    let command_name = git_command_name(&args);
    let output = runner.run(GitCommand {
        repo: repository.to_path_buf(),
        args,
        stdin: None,
        timeout: DEFAULT_TIMEOUT,
    })?;
    if output.status.success() {
        Ok(())
    } else {
        #[cfg(test)]
        eprintln!("E2E fixture Git command failed: {command_name}");
        Err(AppError::Git("E2E fixture Git command failed".to_owned()))
    }
}

fn git_command_name(args: &[OsString]) -> String {
    let mut index = 0;
    while index < args.len() {
        let value = args[index].to_string_lossy();
        if value == "-c" {
            index += 2;
            continue;
        }
        if !value.starts_with('-') {
            return value.into_owned();
        }
        index += 1;
    }
    "unknown".to_owned()
}

fn git_stdout<const N: usize>(
    runner: &SystemGitRunner,
    repository: &Path,
    args: [&str; N],
) -> Result<String, AppError> {
    let output = runner.run(GitCommand {
        repo: repository.to_path_buf(),
        args: args.into_iter().map(OsString::from).collect(),
        stdin: None,
        timeout: DEFAULT_TIMEOUT,
    })?;
    if !output.status.success() {
        return Err(AppError::Git("E2E fixture Git command failed".to_owned()));
    }
    let value = String::from_utf8(output.stdout)
        .map_err(|_| AppError::Git("E2E fixture Git output is not UTF-8".to_owned()))?;
    let value = value.trim();
    if value.is_empty() || value.len() > 128 || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(AppError::Git("E2E fixture Git OID is invalid".to_owned()));
    }
    Ok(value.to_owned())
}

fn is_symlink_or_reparse_point(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    false
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
    if dirty {
        run_git(
            runner,
            repository,
            ["remote", "add", "origin", E2E_PROVIDER_SSH_URL],
        )?;
    }
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
    let args = args.into_iter().map(OsString::from).collect::<Vec<_>>();
    #[cfg(test)]
    let command_name = git_command_name(&args);
    let output = runner.run(GitCommand {
        repo: repository.to_path_buf(),
        args,
        stdin: None,
        timeout: DEFAULT_TIMEOUT,
    })?;
    if output.status.success() {
        Ok(())
    } else {
        #[cfg(test)]
        eprintln!("E2E fixture Git command failed: {command_name}");
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

    use super::{
        E2E_TEMP_PREFIX, E2E_TRANSPORT_PUBLIC_URL, E2eTransportRegistry, cleanup_transport_fixture,
        seed_fixture_files, seed_transport_fixture,
    };
    use crate::git::engine::{DEFAULT_TIMEOUT, GitCommand, GitRunner, SystemGitRunner};
    use crate::git::transport::service::E2eTransportGitRunner;

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

    #[test]
    fn transport_fixture_uses_guarded_temp_paths_and_a_sealed_git_rewrite() {
        let fixture = seed_transport_fixture().expect("Transport fixture seeds");
        assert!(
            fixture
                .root_path
                .file_name()
                .expect("Transport fixture root has a name")
                .to_string_lossy()
                .starts_with("git-ramus-e2e-transport-")
        );
        assert!(fixture.bare_remote.join("HEAD").is_file());
        assert_eq!(
            fixture.public_url,
            "https://gitlab.example.test/skills/private-skill.git"
        );
        let config = fs::read_to_string(&fixture.sealed_global_config)
            .expect("sealed Global Git config reads");
        assert!(config.contains("insteadOf"));
        assert!(!config.contains("provider-pat"));
        cleanup_transport_fixture(&fixture).expect("Transport fixture cleans up");
    }

    #[test]
    fn transport_registry_queues_one_destination_and_rewrites_only_inside_its_runner() {
        let fixture = seed_transport_fixture().expect("Transport fixture seeds");
        let registry = E2eTransportRegistry::default();
        let cleanup_token = uuid::Uuid::new_v4().to_string();
        registry
            .install(fixture, "project-id".to_owned(), cleanup_token.clone())
            .expect("Transport fixture registers");
        let destination = PathBuf::from(
            registry
                .take_destination_parent()
                .expect("destination queue reads")
                .expect("one destination is queued"),
        );
        assert!(
            registry
                .take_destination_parent()
                .expect("destination queue rereads")
                .is_none()
        );
        let clone_path = destination.join("private-skill");
        let runner = E2eTransportGitRunner::new(SystemGitRunner::new(), registry.clone());
        let output = runner
            .run(GitCommand {
                repo: destination,
                args: vec![
                    "clone".into(),
                    "--quiet".into(),
                    "--".into(),
                    E2E_TRANSPORT_PUBLIC_URL.into(),
                    clone_path.as_os_str().to_owned(),
                ],
                stdin: None,
                timeout: DEFAULT_TIMEOUT,
            })
            .expect("rewritten Clone starts");
        assert!(output.status.success());
        assert!(clone_path.join(".git").is_dir());
        registry
            .cleanup(&cleanup_token)
            .expect("registered Transport fixture cleans up");
    }

    #[test]
    fn transport_registry_arms_one_blocked_fetch_without_changing_the_normal_rewrite() {
        let fixture = seed_transport_fixture().expect("Transport fixture seeds");
        let destination = fixture.destination_parent.clone();
        let registry = E2eTransportRegistry::default();
        let cleanup_token = uuid::Uuid::new_v4().to_string();
        registry
            .install(fixture, "project-id".to_owned(), cleanup_token.clone())
            .expect("Transport fixture registers");
        let normal = registry
            .execution_config(&GitCommand {
                repo: destination.clone(),
                args: vec!["fetch".into()],
                stdin: None,
                timeout: DEFAULT_TIMEOUT,
            })
            .expect("normal config resolves")
            .expect("normal config exists");
        registry
            .block_next_fetch(&cleanup_token)
            .expect("blocked Fetch arms");
        assert!(
            registry
                .execution_config(&GitCommand {
                    repo: std::env::temp_dir(),
                    args: vec!["fetch".into()],
                    stdin: None,
                    timeout: DEFAULT_TIMEOUT,
                })
                .expect("foreign Fetch config resolves")
                .is_none()
        );
        let blocked = registry
            .execution_config(&GitCommand {
                repo: destination.clone(),
                args: vec!["fetch".into()],
                stdin: None,
                timeout: DEFAULT_TIMEOUT,
            })
            .expect("blocked config resolves")
            .expect("blocked config exists");
        assert_ne!(blocked.global_config, normal.global_config);
        let blocked_config =
            fs::read_to_string(&blocked.global_config).expect("blocked config reads");
        assert!(blocked_config.contains("http://127.0.0.1:"));
        assert!(!blocked_config.contains("provider-pat"));
        assert!(registry.block_next_fetch(&cleanup_token).is_err());
        let restored = registry
            .execution_config(&GitCommand {
                repo: destination,
                args: vec!["fetch".into()],
                stdin: None,
                timeout: DEFAULT_TIMEOUT,
            })
            .expect("restored config resolves")
            .expect("restored config exists");
        assert_eq!(restored.global_config, normal.global_config);
        registry
            .cleanup(&cleanup_token)
            .expect("blocked Transport fixture cleans up");
    }
}
