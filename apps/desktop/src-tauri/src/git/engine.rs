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

        let mut child = self
            .command(&request)
            .spawn()
            .map_err(|error| AppError::Git(spawn_error_kind(error)))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AppError::Git("Git stdout pipe unavailable".to_owned()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| AppError::Git("Git stderr pipe unavailable".to_owned()))?;
        let stdout_limited = Arc::new(AtomicBool::new(false));
        let stderr_limited = Arc::new(AtomicBool::new(false));
        let stdout_thread = spawn_reader(stdout, self.max_stdout_bytes, stdout_limited.clone());
        let stderr_thread = spawn_reader(stderr, self.max_stderr_bytes, stderr_limited.clone());

        let stdin_thread = request.stdin.map(|input| {
            let mut stdin = child.stdin.take();
            thread::spawn(move || {
                if let Some(mut stdin) = stdin.take() {
                    // A short-lived Git process can close stdin before all bytes are written; a
                    // broken pipe is not itself a security or process failure.
                    let _ = stdin.write_all(&input);
                    let _ = stdin.flush();
                }
            })
        });
        drop(child.stdin.take());

        let (status, timed_out) = wait_bounded(
            &mut child,
            request.timeout,
            self.poll_interval,
            &stdout_limited,
            &stderr_limited,
        )?;
        let stdout = join_reader(stdout_thread)?;
        let stderr = join_reader(stderr_thread)?;
        if let Some(thread) = stdin_thread {
            let _ = thread.join();
        }

        if timed_out {
            return Err(AppError::Timeout);
        }
        if stdout_limited.load(Ordering::Acquire) || stderr_limited.load(Ordering::Acquire) {
            return Err(AppError::OutputLimit);
        }
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
            let _ = child.kill();
            let status = child
                .wait()
                .map_err(|error| AppError::Git(wait_error_kind(error)))?;
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
            let _ = child.kill();
            let status = child
                .wait()
                .map_err(|error| AppError::Git(wait_error_kind(error)))?;
            return Ok((status, timed_out));
        }
        let remaining = timeout.saturating_sub(started.elapsed());
        thread::sleep(poll_interval.min(remaining));
    }
}

fn clean_environment(command: &mut Command) {
    command.env_clear();
    // These values are path/transport settings, not arbitrary user variables. Keeping the
    // allowlist small prevents plugin-provided secrets from being inherited by Git helpers.
    const ALLOWED: &[&str] = &[
        "PATH",
        "HOME",
        "USERPROFILE",
        "HOMEDRIVE",
        "HOMEPATH",
        "SYSTEMROOT",
        "SystemRoot",
        "TEMP",
        "TMP",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
        "GIT_CONFIG_GLOBAL",
        "GIT_CONFIG_SYSTEM",
        "GIT_CONFIG_NOSYSTEM",
        "GIT_SSH",
        "GIT_SSH_VARIANT",
        "GIT_ASKPASS",
        "SSH_AUTH_SOCK",
        "SSH_AGENT_PID",
        "DISPLAY",
        "GCM_INTERACTIVE",
        "GCM_CREDENTIAL_STORE",
    ];
    for key in ALLOWED {
        if let Some(value) = std::env::var_os(key) {
            command.env(OsStr::new(key), value);
        }
    }
    command.env("GIT_TERMINAL_PROMPT", "0");
    command.env("GCM_INTERACTIVE", "Never");
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
