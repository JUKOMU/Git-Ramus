//! A bounded, shell-free process runner for Git.

use std::ffi::{OsStr, OsString};
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::error::AppError;

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
pub const DEFAULT_MAX_STDOUT_BYTES: usize = 8 * 1024 * 1024;
pub const DEFAULT_MAX_STDERR_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct GitCommand {
    pub repo: PathBuf,
    pub args: Vec<OsString>,
    pub stdin: Option<Vec<u8>>,
    pub timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct GitOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub trait GitRunner: Send + Sync {
    fn run(&self, command: GitCommand) -> Result<GitOutput, AppError>;
}

#[derive(Debug, Clone)]
pub struct SystemGitRunner {
    max_stdout_bytes: usize,
    max_stderr_bytes: usize,
    poll_interval: Duration,
}

struct ChildGuard {
    child: Option<Child>,
    reaped: bool,
}

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self {
            child: Some(child),
            reaped: false,
        }
    }

    fn child_mut(&mut self) -> &mut Child {
        self.child.as_mut().expect("child guard must own a process")
    }

    fn mark_reaped(&mut self) {
        self.reaped = true;
    }

    fn terminate(&mut self) {
        if !self.reaped {
            self.reaped = if let Some(child) = self.child.as_mut() {
                terminate_and_reap(child).is_ok()
            } else {
                true
            };
        }
    }

    fn disarm(mut self) {
        self.reaped = true;
        let _ = self.child.take();
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.terminate();
    }
}

impl Default for SystemGitRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemGitRunner {
    pub fn new() -> Self {
        Self {
            max_stdout_bytes: DEFAULT_MAX_STDOUT_BYTES,
            max_stderr_bytes: DEFAULT_MAX_STDERR_BYTES,
            poll_interval: Duration::from_millis(10),
        }
    }

    pub fn with_output_limits(max_stdout_bytes: usize, max_stderr_bytes: usize) -> Self {
        Self {
            max_stdout_bytes,
            max_stderr_bytes,
            ..Self::new()
        }
    }

    /// Alias with a concise name for callers configuring both stream limits.
    pub fn with_limits(max_stdout_bytes: usize, max_stderr_bytes: usize) -> Self {
        Self::with_output_limits(max_stdout_bytes, max_stderr_bytes)
    }

    pub fn max_stdout_bytes(&self) -> usize {
        self.max_stdout_bytes
    }

    pub fn max_stderr_bytes(&self) -> usize {
        self.max_stderr_bytes
    }

    fn command(&self, request: &GitCommand) -> Command {
        let mut command = Command::new("git");
        // `current_dir` is deliberately used instead of composing a shell command. Every
        // caller-supplied argument remains an individual OsString all the way to CreateProcess.
        command
            .current_dir(&request.repo)
            .args(request.args.iter().map(OsString::as_os_str))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        clean_environment(&mut command);
        command
    }
}

impl GitRunner for SystemGitRunner {
    fn run(&self, request: GitCommand) -> Result<GitOutput, AppError> {
        if request.repo.as_os_str().is_empty() {
            return Err(AppError::InvalidInput(
                "repository path is empty".to_owned(),
            ));
        }
        if request.timeout.is_zero() {
            return Err(AppError::InvalidInput(
                "Git timeout must be positive".to_owned(),
            ));
        }

        let child = self
            .command(&request)
            .spawn()
            .map_err(|error| AppError::Git(spawn_error_kind(error)))?;
        let mut guard = ChildGuard::new(child);

        let stdout = match guard.child_mut().stdout.take() {
            Some(stdout) => stdout,
            None => {
                guard.terminate();
                return Err(AppError::Git("Git stdout pipe unavailable".to_owned()));
            }
        };
        let stderr = match guard.child_mut().stderr.take() {
            Some(stderr) => stderr,
            None => {
                guard.terminate();
                return Err(AppError::Git("Git stderr pipe unavailable".to_owned()));
            }
        };
        let stdout_limited = Arc::new(AtomicBool::new(false));
        let stderr_limited = Arc::new(AtomicBool::new(false));
        let stdout_thread = spawn_reader(stdout, self.max_stdout_bytes, stdout_limited.clone());
        let stderr_thread = spawn_reader(stderr, self.max_stderr_bytes, stderr_limited.clone());

        let stdin_thread = request.stdin.map(|input| {
            let mut stdin = guard.child_mut().stdin.take();
            thread::spawn(move || {
                if let Some(mut stdin) = stdin.take() {
                    // A short-lived Git process can close stdin before all bytes are written; a
                    // broken pipe is not itself a security or process failure.
                    let _ = stdin.write_all(&input);
                    let _ = stdin.flush();
                }
            })
        });
        drop(guard.child_mut().stdin.take());

        let wait_result = wait_bounded(
            guard.child_mut(),
            request.timeout,
            self.poll_interval,
            &stdout_limited,
            &stderr_limited,
        );
        let (status, timed_out) = match wait_result {
            Ok(result) => {
                guard.mark_reaped();
                result
            }
            Err(error) => {
                // Ensure all process and pipe resources are closed before returning an error.
                guard.terminate();
                let _ = join_output_threads(stdout_thread, stderr_thread);
                join_stdin_thread(stdin_thread);
                return Err(error);
            }
        };
        let output_result = join_output_threads(stdout_thread, stderr_thread);
        join_stdin_thread(stdin_thread);

        if timed_out {
            return Err(AppError::Timeout);
        }
        if stdout_limited.load(Ordering::Acquire) || stderr_limited.load(Ordering::Acquire) {
            return Err(AppError::OutputLimit);
        }
        let (stdout, stderr) = output_result?;
        guard.disarm();
        Ok(GitOutput {
            status,
            stdout,
            stderr,
        })
    }
}

fn spawn_reader<R>(
    mut reader: R,
    limit: usize,
    exceeded: Arc<AtomicBool>,
) -> JoinHandle<io::Result<Vec<u8>>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut output = Vec::new();
        let mut buffer = [0_u8; 16 * 1024];
        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            if output.len().saturating_add(read) > limit {
                exceeded.store(true, Ordering::Release);
                // Stop retaining bytes immediately. The parent will terminate the child on its
                // next poll; draining a bounded chunk keeps the pipe from deadlocking meanwhile.
                continue;
            }
            output.extend_from_slice(&buffer[..read]);
        }
        Ok(output)
    })
}

fn join_reader(handle: JoinHandle<io::Result<Vec<u8>>>) -> Result<Vec<u8>, AppError> {
    match handle.join() {
        Ok(Ok(bytes)) => Ok(bytes),
        Ok(Err(error)) => Err(AppError::Git(reader_error_kind(error))),
        Err(_) => Err(AppError::Git("Git output reader failed".to_owned())),
    }
}

fn join_output_threads(
    stdout: JoinHandle<io::Result<Vec<u8>>>,
    stderr: JoinHandle<io::Result<Vec<u8>>>,
) -> Result<(Vec<u8>, Vec<u8>), AppError> {
    let stdout = join_reader(stdout);
    let stderr = join_reader(stderr);
    match (stdout, stderr) {
        (Ok(stdout), Ok(stderr)) => Ok((stdout, stderr)),
        (Err(error), _) | (_, Err(error)) => Err(error),
    }
}

fn join_stdin_thread(stdin: Option<JoinHandle<()>>) {
    if let Some(thread) = stdin {
        let _ = thread.join();
    }
}

fn terminate_and_reap(child: &mut Child) -> Result<ExitStatus, AppError> {
    let _ = child.kill();
    child
        .wait()
        .map_err(|error| AppError::Git(wait_error_kind(error)))
}

fn wait_bounded(
    child: &mut Child,
    timeout: Duration,
    poll_interval: Duration,
    stdout_limited: &AtomicBool,
    stderr_limited: &AtomicBool,
) -> Result<(ExitStatus, bool), AppError> {
    let started = Instant::now();
    let mut timed_out = false;
    loop {
        if stdout_limited.load(Ordering::Acquire) || stderr_limited.load(Ordering::Acquire) {
            let status = terminate_and_reap(child)?;
            return Ok((status, false));
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|error| AppError::Git(wait_error_kind(error)))?
        {
            return Ok((status, timed_out));
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            let status = terminate_and_reap(child)?;
            return Ok((status, timed_out));
        }
        let remaining = timeout.saturating_sub(started.elapsed());
        thread::sleep(poll_interval.min(remaining));
    }
}

fn clean_environment(command: &mut Command) {
    clean_environment_from(command, std::env::vars_os());
}

fn clean_environment_from<I>(command: &mut Command, parent: I)
where
    I: IntoIterator<Item = (OsString, OsString)>,
{
    command.env_clear();
    // Keep only path/locale and non-executable SSH-agent transport settings. In particular,
    // never inherit GIT_SSH/GIT_ASKPASS/GIT_CONFIG_* or credential-store overrides from a
    // plugin/parent process.
    for (key, value) in parent {
        if is_safe_inherited_env(&key) {
            command.env(key, value);
        }
    }
    command.env("GIT_TERMINAL_PROMPT", "0");
    command.env("GCM_INTERACTIVE", "Never");
}

fn is_safe_inherited_env(key: &OsStr) -> bool {
    let Some(key) = key.to_str() else {
        return false;
    };
    let normalized = key.to_ascii_uppercase();
    matches!(
        normalized.as_str(),
        "PATH"
            | "HOME"
            | "USERPROFILE"
            | "HOMEDRIVE"
            | "HOMEPATH"
            | "SYSTEMROOT"
            | "TEMP"
            | "TMP"
            | "PATHEXT"
            | "SSH_AUTH_SOCK"
            | "SSH_AGENT_PID"
            | "DISPLAY"
            | "XDG_RUNTIME_DIR"
    ) || normalized == "LANG"
        || normalized.starts_with("LC_")
}

fn spawn_error_kind(error: io::Error) -> String {
    match error.kind() {
        io::ErrorKind::NotFound => "git executable unavailable".to_owned(),
        io::ErrorKind::PermissionDenied => "permission denied starting git".to_owned(),
        _ => "unable to start git".to_owned(),
    }
}

fn reader_error_kind(error: io::Error) -> String {
    let _ = error;
    "unable to read git output".to_owned()
}

fn wait_error_kind(error: io::Error) -> String {
    let _ = error;
    "unable to wait for git".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_environment_drops_parent_git_and_credential_overrides() {
        let mut command = Command::new("git");
        let parent = [
            (OsString::from("GIT_SSH"), OsString::from("evil-ssh")),
            (
                OsString::from("GIT_ASKPASS"),
                OsString::from("evil-askpass"),
            ),
            (
                OsString::from("GIT_CONFIG_GLOBAL"),
                OsString::from("evil-global"),
            ),
            (
                OsString::from("GIT_CONFIG_SYSTEM"),
                OsString::from("evil-system"),
            ),
            (OsString::from("GIT_CONFIG_NOSYSTEM"), OsString::from("0")),
            (
                OsString::from("GCM_CREDENTIAL_STORE"),
                OsString::from("evil-store"),
            ),
            (
                OsString::from("SSH_AUTH_SOCK"),
                OsString::from("agent.sock"),
            ),
        ];
        clean_environment_from(&mut command, parent);
        let names = command
            .get_envs()
            .filter_map(|(key, _)| key.to_str())
            .collect::<Vec<_>>();
        for forbidden in [
            "GIT_SSH",
            "GIT_ASKPASS",
            "GIT_CONFIG_GLOBAL",
            "GIT_CONFIG_SYSTEM",
            "GIT_CONFIG_NOSYSTEM",
            "GCM_CREDENTIAL_STORE",
        ] {
            assert!(!names.contains(&forbidden), "forwarded {forbidden}");
        }
        assert!(names.contains(&"SSH_AUTH_SOCK"));
        assert!(names.contains(&"GIT_TERMINAL_PROMPT"));
        assert!(names.contains(&"GCM_INTERACTIVE"));
    }

    #[test]
    fn terminate_and_reap_always_reaps_child() {
        let Ok(mut child) = Command::new("git")
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            eprintln!("git executable unavailable; skipping cleanup helper test");
            return;
        };
        let _ = terminate_and_reap(&mut child).expect("child can be reaped");
        assert!(child.try_wait().expect("try_wait succeeds").is_some());
    }
}
