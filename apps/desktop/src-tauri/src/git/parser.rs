//! Byte-oriented parsers for Git's machine-readable output.
//!
//! Git paths are not guaranteed to be valid UTF-8.  The status parser therefore keeps NUL
//! records as bytes until each individual path has been decoded, rather than decoding the
//! complete stream through a platform code page.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::model::RepositoryKind;
use crate::error::AppError;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    #[serde(rename = "typeChanged")]
    TypeChanged,
    Untracked,
    Conflicted,
    Unknown,
}

impl ChangeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Modified => "modified",
            Self::Deleted => "deleted",
            Self::Renamed => "renamed",
            Self::Copied => "copied",
            Self::TypeChanged => "typeChanged",
            Self::Untracked => "untracked",
            Self::Conflicted => "conflicted",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for ChangeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str((*self).as_str())
    }
}

impl From<&str> for ChangeKind {
    fn from(value: &str) -> Self {
        match value {
            "added" => Self::Added,
            "modified" => Self::Modified,
            "deleted" => Self::Deleted,
            "renamed" => Self::Renamed,
            "copied" => Self::Copied,
            "typeChanged" | "type_changed" | "typechanged" => Self::TypeChanged,
            "untracked" => Self::Untracked,
            "conflicted" => Self::Conflicted,
            _ => Self::Unknown,
        }
    }
}

impl From<String> for ChangeKind {
    fn from(value: String) -> Self {
        Self::from(value.as_str())
    }
}

impl PartialEq<&str> for ChangeKind {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChangeEntry {
    pub path: String,
    pub original_path: Option<String>,
    pub kind: ChangeKind,
    pub staged: bool,
    pub unstaged: bool,
    pub conflicted: bool,
    pub binary: bool,
    /// For renames/copies, the old path. Kept as an explicit alias for consumers that use a
    /// diff-shaped DTO.
    pub old: Option<String>,
    /// For renames/copies, the new path. Kept as an explicit alias for consumers that use a
    /// diff-shaped DTO.
    pub new: Option<String>,
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    /// The two-character porcelain XY status, when the entry came from status output.
    pub status: String,
    pub index_status: Option<char>,
    pub worktree_status: Option<char>,
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
}

impl ChangeEntry {
    fn status_entry(
        path: String,
        original_path: Option<String>,
        kind: ChangeKind,
        status: [u8; 2],
        conflicted: bool,
    ) -> Self {
        let staged = status[0] != b'.' && status[0] != b'?';
        let unstaged = status[1] != b'.' && status[1] != b'?';
        let old = original_path.clone();
        let new = original_path.as_ref().map(|_| path.clone());
        Self {
            path,
            original_path,
            kind,
            staged,
            unstaged,
            conflicted,
            binary: false,
            old: old.clone(),
            new: new.clone(),
            old_path: old.clone(),
            new_path: new.clone(),
            status: String::from_utf8_lossy(&status).into_owned(),
            index_status: Some(status[0] as char),
            worktree_status: Some(status[1] as char),
            additions: None,
            deletions: None,
        }
    }

    fn untracked(path: String) -> Self {
        Self {
            path,
            original_path: None,
            kind: ChangeKind::Untracked,
            staged: false,
            unstaged: true,
            conflicted: false,
            binary: false,
            old: None,
            new: None,
            old_path: None,
            new_path: None,
            status: "??".to_owned(),
            index_status: Some('?'),
            worktree_status: Some('?'),
            additions: None,
            deletions: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct RepositorySnapshot {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub head_oid: Option<String>,
    /// Alias used by the shared contract terminology.
    pub head_sha: Option<String>,
    pub ahead: u64,
    pub behind: u64,
    pub changes: Vec<ChangeEntry>,
    pub dirty: bool,
    pub is_dirty: bool,
    pub staged_count: usize,
    pub unstaged_count: usize,
    pub untracked_count: usize,
    pub conflicted_count: usize,
    pub total_count: usize,
    pub detached: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiffFile {
    pub path: String,
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub binary: bool,
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
    pub old: Option<String>,
    pub new: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct DiffSummary {
    pub files: Vec<DiffFile>,
    /// `changes` and `entries` are aliases retained for callers that use status terminology.
    pub changes: Vec<DiffFile>,
    pub entries: Vec<DiffFile>,
    pub binary: bool,
    pub additions: u64,
    pub deletions: u64,
}

pub type GitConfig = BTreeMap<String, String>;
type Numstat = (Option<u64>, Option<u64>, String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DetectedRepository {
    pub canonical_path: PathBuf,
    pub git_dir: PathBuf,
    pub kind: RepositoryKind,
    pub is_bare: bool,
    pub is_worktree: bool,
}

/// Parse `git status --porcelain=v2 -z --branch` without lossy decoding.
pub fn parse_status_v2(input: impl AsRef<[u8]>) -> Result<RepositorySnapshot, AppError> {
    let bytes = input.as_ref();
    let records = bytes.split(|byte| *byte == 0).collect::<Vec<_>>();
    let mut snapshot = RepositorySnapshot::default();
    let mut index = 0;

    while index < records.len() {
        let record = records[index];
        index += 1;
        if record.is_empty() {
            continue;
        }

        // Git versions differ slightly in whether branch headers are separated by newlines or
        // NULs when -z is used. Handle both forms while leaving path bytes untouched.
        if record.starts_with(b"# ") && record.contains(&b'\n') {
            for line in record.split(|byte| *byte == b'\n') {
                if !line.is_empty() {
                    parse_branch_header(line, &mut snapshot)?;
                }
            }
            continue;
        }
        if record.starts_with(b"# ") {
            parse_branch_header(record, &mut snapshot)?;
            continue;
        }

        match record.first().copied() {
            Some(b'1') => {
                let fields = split_prefix_fields(record, 8)?;
                let status = two_bytes(fields[1])?;
                let path = decode_path(fields[8])?;
                let conflicted = status_is_conflicted(status);
                let kind = kind_from_status(status, false, conflicted);
                snapshot.changes.push(ChangeEntry::status_entry(
                    path, None, kind, status, conflicted,
                ));
            }
            Some(b'2') => {
                let fields = split_prefix_fields(record, 9)?;
                let status = two_bytes(fields[1])?;
                let path = decode_path(fields[9])?;
                let original = records
                    .get(index)
                    .ok_or_else(|| invalid_status("rename record is missing its original path"))?;
                index += 1;
                let original = decode_path(original)?;
                let conflicted = status_is_conflicted(status);
                let kind = if status[0] == b'C' || status[1] == b'C' {
                    ChangeKind::Copied
                } else {
                    ChangeKind::Renamed
                };
                snapshot.changes.push(ChangeEntry::status_entry(
                    path,
                    Some(original),
                    kind,
                    status,
                    conflicted,
                ));
            }
            Some(b'u') => {
                let fields = split_prefix_fields(record, 10)?;
                let status = two_bytes(fields[1])?;
                let path = decode_path(fields[10])?;
                snapshot.changes.push(ChangeEntry::status_entry(
                    path,
                    None,
                    ChangeKind::Conflicted,
                    status,
                    true,
                ));
            }
            Some(b'?') => {
                if record.len() < 3 || record[1] != b' ' {
                    return Err(invalid_status("malformed untracked record"));
                }
                snapshot
                    .changes
                    .push(ChangeEntry::untracked(decode_path(&record[2..])?));
            }
            Some(b'!') => {
                // Ignored files are intentionally not changes in the public snapshot.
            }
            _ => return Err(invalid_status("unknown porcelain v2 record")),
        }
    }

    snapshot.staged_count = snapshot.changes.iter().filter(|entry| entry.staged).count();
    // Untracked files have their own count in the public summary and are not included in the
    // unstaged counter, even though they are naturally present in the worktree.
    snapshot.unstaged_count = snapshot
        .changes
        .iter()
        .filter(|entry| entry.unstaged && entry.kind != ChangeKind::Untracked)
        .count();
    snapshot.untracked_count = snapshot
        .changes
        .iter()
        .filter(|entry| entry.kind == ChangeKind::Untracked)
        .count();
    snapshot.conflicted_count = snapshot
        .changes
        .iter()
        .filter(|entry| entry.conflicted)
        .count();
    snapshot.total_count = snapshot.changes.len();
    snapshot.dirty = !snapshot.changes.is_empty();
    snapshot.is_dirty = snapshot.dirty;
    snapshot.head_sha = snapshot.head_oid.clone();
    Ok(snapshot)
}

fn parse_branch_header(record: &[u8], snapshot: &mut RepositorySnapshot) -> Result<(), AppError> {
    let text = std::str::from_utf8(record).map_err(|_| invalid_path())?;
    let Some(value) = text.strip_prefix("# ") else {
        return Err(invalid_status("malformed branch header"));
    };
    if let Some(oid) = value.strip_prefix("branch.oid ") {
        if !matches!(oid, "(initial)" | "(unknown)") {
            snapshot.head_oid = Some(oid.to_owned());
        }
    } else if let Some(branch) = value.strip_prefix("branch.head ") {
        if branch == "(detached)" {
            snapshot.detached = true;
            snapshot.branch = None;
        } else {
            snapshot.branch = Some(branch.to_owned());
        }
    } else if let Some(upstream) = value.strip_prefix("branch.upstream ") {
        if upstream != "(null)" {
            snapshot.upstream = Some(upstream.to_owned());
        }
    } else if let Some(ab) = value.strip_prefix("branch.ab ") {
        let mut parts = ab.split_whitespace();
        snapshot.ahead = parse_signed_count(parts.next(), '+')?;
        snapshot.behind = parse_signed_count(parts.next(), '-')?;
    }
    Ok(())
}

fn parse_signed_count(value: Option<&str>, sign: char) -> Result<u64, AppError> {
    let Some(value) = value else {
        return Err(invalid_status("malformed branch ahead/behind header"));
    };
    let Some(number) = value.strip_prefix(sign) else {
        return Err(invalid_status("malformed branch ahead/behind count"));
    };
    number
        .parse::<u64>()
        .map_err(|_| invalid_status("malformed branch ahead/behind count"))
}

fn split_prefix_fields(record: &[u8], spaces: usize) -> Result<Vec<&[u8]>, AppError> {
    let mut fields = Vec::with_capacity(spaces + 1);
    let mut start = 0;
    for _ in 0..spaces {
        let Some(relative) = record[start..].iter().position(|byte| *byte == b' ') else {
            return Err(invalid_status("malformed porcelain v2 record"));
        };
        let split = start + relative;
        fields.push(&record[start..split]);
        start = split + 1;
    }
    fields.push(&record[start..]);
    Ok(fields)
}

fn two_bytes(field: &[u8]) -> Result<[u8; 2], AppError> {
    if field.len() != 2 {
        return Err(invalid_status("malformed porcelain status code"));
    }
    Ok([field[0], field[1]])
}

fn status_is_conflicted(status: [u8; 2]) -> bool {
    matches!(status[0], b'U' | b'A' if status[1] == b'U')
        || matches!(status[1], b'U')
        || matches!(status[0], b'U')
}

fn kind_from_status(status: [u8; 2], rename: bool, conflicted: bool) -> ChangeKind {
    if conflicted {
        return ChangeKind::Conflicted;
    }
    if rename {
        return ChangeKind::Renamed;
    }
    let code = if status[0] != b'.' {
        status[0]
    } else {
        status[1]
    };
    match code {
        b'A' => ChangeKind::Added,
        b'D' => ChangeKind::Deleted,
        b'M' => ChangeKind::Modified,
        b'R' => ChangeKind::Renamed,
        b'C' => ChangeKind::Copied,
        b'T' => ChangeKind::TypeChanged,
        _ => ChangeKind::Unknown,
    }
}

fn decode_path(path: &[u8]) -> Result<String, AppError> {
    if path.is_empty() {
        return Err(invalid_status("empty path in Git output"));
    }
    std::str::from_utf8(path)
        .map(str::to_owned)
        .map_err(|_| invalid_path())
}

fn invalid_status(message: &str) -> AppError {
    AppError::InvalidInput(message.to_owned())
}

/// Parse a bounded diff summary. No full patch text is retained in the returned value.
pub fn parse_diff_summary(input: impl AsRef<[u8]>) -> Result<DiffSummary, AppError> {
    let bytes = input.as_ref();
    let mut files = Vec::<DiffFile>::new();
    let mut current: Option<usize> = None;
    for line in bytes.split(|byte| *byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.starts_with(b"diff --git ") {
            let (old_path, new_path) = parse_diff_git_header(&line[11..])?;
            let path = new_path
                .clone()
                .or_else(|| old_path.clone())
                .ok_or_else(|| {
                    AppError::InvalidInput("diff header does not contain a path".to_owned())
                })?;
            files.push(DiffFile {
                path,
                old_path,
                new_path,
                binary: false,
                additions: None,
                deletions: None,
                old: None,
                new: None,
            });
            current = Some(files.len() - 1);
        } else if line.starts_with(b"--- ") {
            let path = parse_patch_path(&line[4..])?;
            if let Some(index) = current {
                files[index].old_path = path.clone();
                files[index].old = path;
            } else if let Some(path) = path {
                files.push(new_diff_file(path, None));
                current = Some(files.len() - 1);
            }
        } else if line.starts_with(b"+++ ") {
            let path = parse_patch_path(&line[4..])?;
            if let Some(index) = current {
                if let Some(path) = path {
                    files[index].new_path = Some(path.clone());
                    files[index].path = path.clone();
                    files[index].new = Some(path);
                }
            }
        } else if line.starts_with(b"Binary files ") || line.starts_with(b"GIT binary patch") {
            let index = ensure_diff_file(&mut files, &mut current);
            files[index].binary = true;
        } else if line.starts_with(b"rename from ") {
            let path = decode_path(&line[12..])?;
            let index = ensure_diff_file(&mut files, &mut current);
            files[index].old_path = Some(path.clone());
            files[index].old = Some(path);
        } else if line.starts_with(b"rename to ") {
            let path = decode_path(&line[10..])?;
            let index = ensure_diff_file(&mut files, &mut current);
            files[index].new_path = Some(path.clone());
            files[index].path = path.clone();
            files[index].new = Some(path);
        } else if let Some((additions, deletions, path)) = parse_numstat(line)? {
            let index = ensure_diff_file(&mut files, &mut current);
            files[index].additions = additions;
            files[index].deletions = deletions;
            if !path.is_empty() {
                files[index].path = path;
            }
            if additions.is_none() && deletions.is_none() {
                files[index].binary = true;
            }
        }
    }

    let binary = files.iter().any(|file| file.binary);
    let additions = files.iter().filter_map(|file| file.additions).sum();
    let deletions = files.iter().filter_map(|file| file.deletions).sum();
    Ok(DiffSummary {
        changes: files.clone(),
        entries: files.clone(),
        files,
        binary,
        additions,
        deletions,
    })
}

fn new_diff_file(path: String, old_path: Option<String>) -> DiffFile {
    DiffFile {
        path: path.clone(),
        old_path: old_path.clone(),
        new_path: Some(path.clone()),
        binary: false,
        additions: None,
        deletions: None,
        old: old_path,
        new: Some(path),
    }
}

fn ensure_diff_file(files: &mut Vec<DiffFile>, current: &mut Option<usize>) -> usize {
    if let Some(index) = *current {
        index
    } else {
        files.push(new_diff_file(String::new(), None));
        let index = files.len() - 1;
        *current = Some(index);
        index
    }
}

fn parse_numstat(line: &[u8]) -> Result<Option<Numstat>, AppError> {
    let Some(first_tab) = line.iter().position(|byte| *byte == b'\t') else {
        return Ok(None);
    };
    let rest = &line[first_tab + 1..];
    let Some(second_tab) = rest.iter().position(|byte| *byte == b'\t') else {
        return Ok(None);
    };
    let additions_field = &line[..first_tab];
    if additions_field != b"-" && !additions_field.iter().all(|byte| byte.is_ascii_digit()) {
        return Ok(None);
    }
    let additions = parse_count(additions_field)?;
    let deletions = parse_count(&rest[..second_tab])?;
    let path = decode_path(&rest[second_tab + 1..])?;
    Ok(Some((additions, deletions, path)))
}

fn parse_count(value: &[u8]) -> Result<Option<u64>, AppError> {
    if value == b"-" {
        return Ok(None);
    }
    let text = std::str::from_utf8(value)
        .map_err(|_| AppError::InvalidInput("invalid diff count".to_owned()))?;
    text.parse::<u64>()
        .map(Some)
        .map_err(|_| AppError::InvalidInput("invalid diff count".to_owned()))
}

fn parse_diff_git_header(header: &[u8]) -> Result<(Option<String>, Option<String>), AppError> {
    // Git normally quotes paths containing spaces, but a few producers emit the raw
    // `a/path b/path` form. Split at the structural ` b/` separator before tokenizing so spaces
    // inside either path are retained.
    if header.starts_with(b"a/") {
        if let Some(separator) = header.windows(3).position(|window| window == b" b/") {
            let old = strip_diff_prefix(&header[..separator]);
            let new = strip_diff_prefix(&header[separator + 1..]);
            if old.is_some() && new.is_some() {
                return Ok((old, new));
            }
        }
    }
    let tokens = parse_git_path_tokens(header)?;
    if tokens.len() < 2 {
        return Err(AppError::InvalidInput("malformed diff header".to_owned()));
    }
    let old = strip_diff_prefix(&tokens[0]);
    let new = strip_diff_prefix(&tokens[1]);
    Ok((old, new))
}

fn parse_git_path_tokens(value: &[u8]) -> Result<Vec<Vec<u8>>, AppError> {
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < value.len() {
        while index < value.len() && value[index] == b' ' {
            index += 1;
        }
        if index == value.len() {
            break;
        }
        if value[index] == b'"' {
            index += 1;
            let mut token = Vec::new();
            while index < value.len() {
                match value[index] {
                    b'"' => {
                        index += 1;
                        break;
                    }
                    b'\\' if index + 1 < value.len() => {
                        index += 1;
                        token.push(value[index]);
                        index += 1;
                    }
                    byte => {
                        token.push(byte);
                        index += 1;
                    }
                }
            }
            tokens.push(token);
        } else {
            let start = index;
            while index < value.len() && value[index] != b' ' {
                index += 1;
            }
            tokens.push(value[start..index].to_vec());
        }
    }
    Ok(tokens)
}

fn strip_diff_prefix(path: &[u8]) -> Option<String> {
    if path == b"/dev/null" {
        return None;
    }
    let path = path
        .strip_prefix(b"a/")
        .or_else(|| path.strip_prefix(b"b/"))
        .unwrap_or(path);
    std::str::from_utf8(path).ok().map(str::to_owned)
}

fn parse_patch_path(path: &[u8]) -> Result<Option<String>, AppError> {
    let path = path.split(|byte| *byte == b'\t').next().unwrap_or(path);
    if path == b"/dev/null" {
        return Ok(None);
    }
    let stripped = path
        .strip_prefix(b"a/")
        .or_else(|| path.strip_prefix(b"b/"))
        .unwrap_or(path);
    Ok(Some(decode_path(stripped)?))
}

/// Parse NUL-terminated Git config output. Both `key\nvalue\0` (get-regexp) and
/// `key\0value\0` (list) forms are accepted.
pub fn parse_git_config(input: impl AsRef<[u8]>) -> Result<GitConfig, AppError> {
    let mut records = input.as_ref().split(|byte| *byte == 0).collect::<Vec<_>>();
    // A trailing NUL contributes an empty sentinel; interior empty records are meaningful (for
    // example, a config key with an explicitly empty value) and must be retained.
    if records.last().is_some_and(|record| record.is_empty()) {
        records.pop();
    }
    let mut config = BTreeMap::new();
    let mut index = 0;
    while index < records.len() {
        let record = records[index];
        if let Some(separator) = record
            .iter()
            .position(|byte| *byte == b'\n' || *byte == b'\t' || *byte == b'=')
        {
            let key = decode_config_key(&record[..separator])?;
            let value = decode_config_value(&record[separator + 1..])?;
            config.insert(key, value);
            index += 1;
        } else {
            let value = records
                .get(index + 1)
                .ok_or_else(|| AppError::InvalidInput("unpaired Git config key".to_owned()))?;
            config.insert(decode_config_key(record)?, decode_config_value(value)?);
            index += 2;
        }
    }
    Ok(config)
}

/// Canonicalize a path and classify a normal, bare, or linked worktree repository.
pub fn detect_repository(path: impl AsRef<Path>) -> Result<DetectedRepository, AppError> {
    let input = path.as_ref();
    if input.as_os_str().is_empty() {
        return Err(AppError::InvalidInput(
            "repository path is empty".to_owned(),
        ));
    }
    let canonical = fs::canonicalize(input).map_err(|_| invalid_repository())?;
    if canonical.to_str().is_none() {
        return Err(invalid_path());
    }
    let metadata = fs::metadata(&canonical).map_err(|_| invalid_repository())?;
    if !metadata.is_dir() {
        return Err(invalid_repository());
    }

    // A caller may pass a directory below a repository root. Walk upward until a marker is
    // found, returning the canonical repository root rather than the nested working directory.
    let mut cursor = Some(canonical.as_path());
    while let Some(candidate) = cursor {
        let dot_git = candidate.join(".git");
        if dot_git.is_dir() {
            return Ok(DetectedRepository {
                canonical_path: candidate.to_path_buf(),
                git_dir: dot_git,
                kind: RepositoryKind::Normal,
                is_bare: false,
                is_worktree: false,
            });
        }
        if dot_git.is_file() {
            let git_dir = read_worktree_gitdir(candidate, &dot_git)?;
            return Ok(DetectedRepository {
                canonical_path: candidate.to_path_buf(),
                git_dir,
                kind: RepositoryKind::Worktree,
                is_bare: false,
                is_worktree: true,
            });
        }
        cursor = candidate.parent();
    }

    if is_bare_repository(&canonical) {
        return Ok(DetectedRepository {
            canonical_path: canonical.clone(),
            git_dir: canonical,
            kind: RepositoryKind::Bare,
            is_bare: true,
            is_worktree: false,
        });
    }
    Err(invalid_repository())
}

fn read_worktree_gitdir(root: &Path, marker: &Path) -> Result<PathBuf, AppError> {
    let contents = fs::read(marker).map_err(|_| invalid_repository())?;
    let text = std::str::from_utf8(&contents).map_err(|_| invalid_path())?;
    let value = text
        .lines()
        .find_map(|line| line.strip_prefix("gitdir:"))
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .ok_or_else(invalid_repository)?;
    let git_dir = Path::new(value);
    let git_dir = if git_dir.is_absolute() {
        git_dir.to_path_buf()
    } else {
        root.join(git_dir)
    };
    fs::canonicalize(git_dir).map_err(|_| invalid_repository())
}

fn is_bare_repository(path: &Path) -> bool {
    let config = path.join("config");
    if let Ok(bytes) = fs::read(config) {
        let text = String::from_utf8_lossy(&bytes).to_ascii_lowercase();
        if text.lines().any(|line| {
            let normalized = line.trim().replace(' ', "");
            normalized == "bare=true" || normalized == "bare=1"
        }) {
            return true;
        }
    }
    path.join("HEAD").is_file() && path.join("objects").is_dir() && path.join("refs").is_dir()
}

fn invalid_repository() -> AppError {
    AppError::InvalidInput("path is not a Git repository".to_owned())
}

fn invalid_path() -> AppError {
    AppError::InvalidInput("path is not valid UTF-8".to_owned())
}

fn decode_config_key(value: &[u8]) -> Result<String, AppError> {
    if value.is_empty() {
        return Err(AppError::InvalidInput("empty Git config key".to_owned()));
    }
    decode_path(value)
}

fn decode_config_value(value: &[u8]) -> Result<String, AppError> {
    std::str::from_utf8(value)
        .map(str::to_owned)
        .map_err(|_| invalid_path())
}
