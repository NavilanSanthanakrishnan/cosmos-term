use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use ring::pbkdf2;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CURRENT_LAYOUT_VERSION: u8 = 4;
pub const DEFAULT_SIDEBAR_WIDTH: usize = 520;
pub const MIN_SIDEBAR_WIDTH: usize = 240;
pub const MAX_SIDEBAR_WIDTH: usize = 840;
pub const MAX_DIRECTORY_ENTRIES: usize = 5_000;
pub const MAX_FILE_SEARCH_ENTRIES: usize = 20_000;
pub const MAX_FILE_SEARCH_RESULTS: usize = 200;
pub const MAX_EDITABLE_FILE_BYTES: u64 = 2 * 1024 * 1024;
const CODEX_ROLLOUT_INITIAL_TAIL_BYTES: u64 = 512 * 1024;
const CODEX_ROLLOUT_MAX_TAIL_BYTES: u64 = 8 * 1024 * 1024;
const CODEX_ROLLOUT_DISCOVERY_INTERVAL: Duration = Duration::from_secs(15);
const CODEX_ROLLOUT_CANDIDATE_LIMIT: usize = 16;
#[cfg(target_os = "macos")]
const SYSTEM_CAPACITY_REFRESH_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Deserialize)]
struct CloseLockCredential {
    kdf: String,
    iterations: u32,
    salt: String,
    digest: String,
}

pub fn verify_close_lock(path: &Path, passphrase: &str) -> io::Result<bool> {
    let data = fs::read(path)?;
    verify_close_lock_credential(&data, passphrase)
}

fn verify_close_lock_credential(data: &[u8], passphrase: &str) -> io::Result<bool> {
    if passphrase.is_empty() {
        return Ok(false);
    }
    let credential: CloseLockCredential = serde_json::from_slice(data)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if credential.kdf != "pbkdf2-hmac-sha256" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported close-lock credential",
        ));
    }
    let iterations = NonZeroU32::new(credential.iterations).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "close-lock iterations must be nonzero",
        )
    })?;
    let salt = BASE64_STANDARD
        .decode(credential.salt)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let expected = BASE64_STANDARD
        .decode(credential.digest)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok(pbkdf2::verify(
        pbkdf2::PBKDF2_HMAC_SHA256,
        iterations,
        &salt,
        passphrase.as_bytes(),
        &expected,
    )
    .is_ok())
}

fn default_true() -> bool {
    true
}

fn default_width() -> usize {
    DEFAULT_SIDEBAR_WIDTH
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FollowMode {
    Follow,
    ProjectFollow,
    Locked,
}

impl Default for FollowMode {
    fn default() -> Self {
        Self::Follow
    }
}

impl FollowMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Follow => "FOLLOW",
            Self::ProjectFollow => "PROJECT",
            Self::Locked => "LOCKED",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Follow => Self::ProjectFollow,
            Self::ProjectFollow => Self::Locked,
            Self::Locked => Self::Follow,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceRoot {
    pub path: PathBuf,
    pub name: String,
}

impl WorkspaceRoot {
    pub fn new(path: PathBuf) -> Self {
        let name = path
            .file_name()
            .filter(|name| !name.is_empty())
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        Self { path, name }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExplorerState {
    #[serde(default)]
    pub layout_version: u8,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default = "default_width")]
    pub width_px: usize,
    #[serde(default)]
    pub roots: Vec<WorkspaceRoot>,
    #[serde(default)]
    pub expanded: BTreeSet<PathBuf>,
    #[serde(default)]
    pub follow_mode: FollowMode,
    #[serde(default)]
    pub show_hidden: bool,
}

impl Default for ExplorerState {
    fn default() -> Self {
        Self {
            layout_version: CURRENT_LAYOUT_VERSION,
            visible: true,
            width_px: DEFAULT_SIDEBAR_WIDTH,
            roots: vec![],
            expanded: BTreeSet::new(),
            follow_mode: FollowMode::Follow,
            show_hidden: true,
        }
    }
}

impl ExplorerState {
    pub fn load(path: &Path) -> io::Result<Self> {
        let data = match fs::read(path) {
            Ok(data) => data,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => return Err(err),
        };
        let mut state: Self = serde_json::from_slice(&data)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        if state.layout_version < CURRENT_LAYOUT_VERSION {
            state.layout_version = CURRENT_LAYOUT_VERSION;
            state.visible = true;
            state.width_px = DEFAULT_SIDEBAR_WIDTH;
            state.show_hidden = true;
            state.follow_mode = FollowMode::Follow;
        }
        state.width_px = state.width_px.clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
        state.deduplicate_roots();
        Ok(state)
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_vec_pretty(self)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        let temporary = path.with_extension("json.tmp");
        fs::write(&temporary, data)?;
        fs::rename(temporary, path)
    }

    pub fn add_root(&mut self, path: PathBuf) -> usize {
        let path = normalize_existing_path(path);
        if let Some(index) = self.roots.iter().position(|root| root.path == path) {
            return index;
        }
        self.roots.push(WorkspaceRoot::new(path));
        self.roots.len() - 1
    }

    pub fn remove_root(&mut self, index: usize) -> Option<WorkspaceRoot> {
        if index >= self.roots.len() {
            return None;
        }
        let root = self.roots.remove(index);
        self.expanded.retain(|path| !path.starts_with(&root.path));
        Some(root)
    }

    pub fn move_root(&mut self, index: usize, delta: isize) -> Option<usize> {
        if index >= self.roots.len() {
            return None;
        }
        let destination =
            (index as isize + delta).clamp(0, self.roots.len().saturating_sub(1) as isize) as usize;
        if destination != index {
            let root = self.roots.remove(index);
            self.roots.insert(destination, root);
        }
        Some(destination)
    }

    pub fn rename_root(&mut self, index: usize, name: String) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        match self.roots.get_mut(index) {
            Some(root) => {
                root.name = name.to_string();
                true
            }
            None => false,
        }
    }

    pub fn matching_root(&self, path: &Path) -> Option<usize> {
        self.roots
            .iter()
            .enumerate()
            .filter(|(_, root)| path.starts_with(&root.path))
            .max_by_key(|(_, root)| root.path.components().count())
            .map(|(index, _)| index)
    }

    pub fn ensure_root_for_path(&mut self, path: &Path) -> usize {
        if let Some(index) = self.matching_root(path) {
            return index;
        }
        self.add_root(find_project_root(path).unwrap_or_else(|| path.to_path_buf()))
    }

    pub fn reveal_path(&mut self, path: &Path, project_root: Option<&Path>) -> usize {
        let root_index = self.ensure_root_for_path(path);
        let root_path = self.roots[root_index].path.clone();
        match self.follow_mode {
            FollowMode::Follow => {
                for ancestor in ancestors_from_root(&root_path, path) {
                    self.expanded.insert(ancestor);
                }
            }
            FollowMode::ProjectFollow => {
                self.expanded.insert(root_path.clone());
                if let Some(project_root) = project_root {
                    for ancestor in ancestors_from_root(&root_path, project_root) {
                        self.expanded.insert(ancestor);
                    }
                }
            }
            FollowMode::Locked => {}
        }
        root_index
    }

    fn deduplicate_roots(&mut self) {
        let mut seen = HashSet::new();
        self.roots
            .retain(|root| seen.insert(normalize_existing_path(root.path.clone())));
        for root in &mut self.roots {
            root.path = normalize_existing_path(root.path.clone());
            if root.name.trim().is_empty() {
                root.name = WorkspaceRoot::new(root.path.clone()).name;
            }
        }
    }
}

pub fn expand_home(path: &str, home: &Path) -> PathBuf {
    if path == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return home.join(rest);
    }
    PathBuf::from(path)
}

pub fn normalize_existing_path(path: PathBuf) -> PathBuf {
    fs::canonicalize(&path).unwrap_or(path)
}

pub fn find_project_root(path: &Path) -> Option<PathBuf> {
    let mut current = if path.is_dir() {
        Some(path)
    } else {
        path.parent()
    };
    while let Some(candidate) = current {
        if candidate.join(".git").exists() {
            return Some(candidate.to_path_buf());
        }
        current = candidate.parent();
    }
    None
}

pub fn ancestors_from_root(root: &Path, target: &Path) -> Vec<PathBuf> {
    if !target.starts_with(root) {
        return vec![];
    }
    let mut result = vec![root.to_path_buf()];
    let relative = match target.strip_prefix(root) {
        Ok(relative) => relative,
        Err(_) => return result,
    };
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        result.push(current.clone());
    }
    result
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub is_symlink: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryListing {
    pub path: PathBuf,
    pub entries: Vec<DirectoryEntry>,
    pub error: Option<String>,
    pub truncated: usize,
    pub modified: Option<SystemTime>,
}

impl DirectoryListing {
    pub fn read(path: &Path, show_hidden: bool) -> Self {
        let modified = fs::metadata(path).and_then(|meta| meta.modified()).ok();
        let mut entries = vec![];
        let mut error = None;
        let mut truncated = 0;
        match fs::read_dir(path) {
            Ok(read_dir) => {
                for item in read_dir {
                    let item = match item {
                        Ok(item) => item,
                        Err(err) => {
                            error = Some(err.to_string());
                            continue;
                        }
                    };
                    let name = item.file_name().to_string_lossy().to_string();
                    if matches!(name.as_str(), ".git" | ".DS_Store") {
                        continue;
                    }
                    if !show_hidden && name.starts_with('.') {
                        continue;
                    }
                    let file_type = match item.file_type() {
                        Ok(file_type) => file_type,
                        Err(err) => {
                            error = Some(err.to_string());
                            continue;
                        }
                    };
                    if entries.len() >= MAX_DIRECTORY_ENTRIES {
                        truncated += 1;
                        continue;
                    }
                    entries.push(DirectoryEntry {
                        path: item.path(),
                        name,
                        is_dir: file_type.is_dir(),
                        is_symlink: file_type.is_symlink(),
                    });
                }
            }
            Err(err) => error = Some(err.to_string()),
        }
        entries.sort_by(|left, right| {
            right
                .is_dir
                .cmp(&left.is_dir)
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
                .then_with(|| left.name.cmp(&right.name))
        });
        Self {
            path: path.to_path_buf(),
            entries,
            error,
            truncated,
            modified,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerRowKind {
    Root,
    Directory,
    File,
    Loading,
    Error,
    Truncated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerRow {
    pub path: Option<PathBuf>,
    pub root_index: usize,
    pub depth: usize,
    pub label: String,
    pub kind: ExplorerRowKind,
    pub expanded: bool,
}

#[derive(Debug, Default)]
pub struct DirectoryCache {
    listings: HashMap<PathBuf, DirectoryListing>,
    loading: HashSet<PathBuf>,
}

impl DirectoryCache {
    pub fn is_loaded(&self, path: &Path) -> bool {
        self.listings.contains_key(path)
    }

    pub fn mark_loading(&mut self, path: PathBuf) -> bool {
        self.loading.insert(path)
    }

    pub fn apply(&mut self, listing: DirectoryListing) -> bool {
        self.loading.remove(&listing.path);
        let changed = self
            .listings
            .get(&listing.path)
            .map(|prior| prior != &listing)
            .unwrap_or(true);
        self.listings.insert(listing.path.clone(), listing);
        changed
    }

    pub fn clear(&mut self) {
        self.listings.clear();
        self.loading.clear();
    }

    pub fn rows(&self, state: &ExplorerState) -> Vec<ExplorerRow> {
        let mut rows = vec![];
        for (root_index, root) in state.roots.iter().enumerate() {
            self.append_root(state, root, root_index, &mut rows);
        }
        rows
    }

    /// Build the explorer tree for a single visible root. This is used by the
    /// UI's folder-scoped Follow modes so saved multi-root workspace state does
    /// not leak parent or sibling directories into the current view.
    pub fn rows_for_root(
        &self,
        state: &ExplorerState,
        root: &WorkspaceRoot,
        root_index: usize,
    ) -> Vec<ExplorerRow> {
        let mut rows = vec![];
        self.append_root(state, root, root_index, &mut rows);
        rows
    }

    fn append_root(
        &self,
        state: &ExplorerState,
        root: &WorkspaceRoot,
        root_index: usize,
        rows: &mut Vec<ExplorerRow>,
    ) {
        let expanded = state.expanded.contains(&root.path);
        rows.push(ExplorerRow {
            path: Some(root.path.clone()),
            root_index,
            depth: 0,
            label: root.name.clone(),
            kind: ExplorerRowKind::Root,
            expanded,
        });
        if expanded {
            self.append_children(state, root_index, &root.path, 1, rows);
        }
    }

    fn append_children(
        &self,
        state: &ExplorerState,
        root_index: usize,
        parent: &Path,
        depth: usize,
        rows: &mut Vec<ExplorerRow>,
    ) {
        let listing = match self.listings.get(parent) {
            Some(listing) => listing,
            None => {
                rows.push(ExplorerRow {
                    path: None,
                    root_index,
                    depth,
                    label: if self.loading.contains(parent) {
                        "Loading…".to_string()
                    } else {
                        "Not loaded".to_string()
                    },
                    kind: ExplorerRowKind::Loading,
                    expanded: false,
                });
                return;
            }
        };
        for entry in &listing.entries {
            let expanded = entry.is_dir && state.expanded.contains(&entry.path);
            rows.push(ExplorerRow {
                path: Some(entry.path.clone()),
                root_index,
                depth,
                label: entry.name.clone(),
                kind: if entry.is_dir {
                    ExplorerRowKind::Directory
                } else {
                    ExplorerRowKind::File
                },
                expanded,
            });
            if expanded {
                self.append_children(state, root_index, &entry.path, depth + 1, rows);
            }
        }
        if let Some(error) = &listing.error {
            rows.push(ExplorerRow {
                path: Some(parent.to_path_buf()),
                root_index,
                depth,
                label: error.clone(),
                kind: ExplorerRowKind::Error,
                expanded: false,
            });
        }
        if listing.truncated > 0 {
            rows.push(ExplorerRow {
                path: Some(parent.to_path_buf()),
                root_index,
                depth,
                label: format!("{} additional entries hidden", listing.truncated),
                kind: ExplorerRowKind::Truncated,
                expanded: false,
            });
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextSource {
    Native,
    Tmux,
    LastKnown,
    Unknown,
}

impl ContextSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Tmux => "tmux",
            Self::LastKnown => "stale",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PaneContextRequest {
    pub pane_id: usize,
    pub pane_title: String,
    pub reported_cwd: Option<PathBuf>,
    pub foreground_process: Option<String>,
    pub tty_name: Option<String>,
    pub roots: Vec<PathBuf>,
    pub last_known_cwd: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneContext {
    pub pane_id: usize,
    pub pane_title: String,
    pub cwd: Option<PathBuf>,
    pub project_root: Option<PathBuf>,
    pub workspace_root: Option<PathBuf>,
    pub foreground_process: Option<String>,
    pub source: ContextSource,
    pub tmux_geometry: Option<TmuxPaneGeometry>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TmuxPaneGeometry {
    pub left: usize,
    pub top: usize,
    pub width: usize,
    pub height: usize,
}

impl PaneContext {
    pub fn resolve(request: PaneContextRequest) -> Self {
        let mut cwd = None;
        let mut source = ContextSource::Unknown;
        let mut tmux_geometry = None;
        let mut error = None;

        if process_is_tmux(request.foreground_process.as_deref()) {
            if let Some(tty_name) = request.tty_name.as_deref() {
                match tmux_pane_context(
                    tty_name,
                    request.foreground_process.as_deref().unwrap_or("tmux"),
                ) {
                    Ok(context) => {
                        tmux_geometry = Some(context.geometry);
                        let path = context.path;
                        cwd = Some(path);
                        source = ContextSource::Tmux;
                    }
                    Err(err) => error = Some(err),
                }
            }
        }

        if cwd.is_none() {
            if let Some(path) = request.reported_cwd {
                cwd = Some(path);
                source = ContextSource::Native;
            } else if let Some(path) = request.last_known_cwd {
                cwd = Some(path);
                source = ContextSource::LastKnown;
            }
        }

        let cwd = cwd.map(normalize_existing_path);
        let project_root = cwd.as_deref().and_then(find_project_root);
        let workspace_root = cwd.as_deref().and_then(|cwd| {
            request
                .roots
                .iter()
                .filter(|root| cwd.starts_with(root))
                .max_by_key(|root| root.components().count())
                .cloned()
                .or_else(|| project_root.clone())
        });

        Self {
            pane_id: request.pane_id,
            pane_title: request.pane_title,
            cwd,
            project_root,
            workspace_root,
            foreground_process: request.foreground_process,
            source,
            tmux_geometry,
            error,
        }
    }
}

pub fn process_is_tmux(process: Option<&str>) -> bool {
    process
        .and_then(|process| Path::new(process).file_name())
        .map(|name| name == OsStr::new("tmux"))
        .unwrap_or(false)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxPaneContext {
    pub path: PathBuf,
    pub geometry: TmuxPaneGeometry,
}

fn parse_tmux_pane_context(value: &str) -> Result<TmuxPaneContext, String> {
    let mut fields = value.trim_end_matches(['\r', '\n']).split('\u{1f}');
    let path = fields.next().unwrap_or_default();
    if path.is_empty() {
        return Err("tmux returned an empty pane path".to_string());
    }
    let parse_dimension = |name: &str, value: Option<&str>| {
        value
            .ok_or_else(|| format!("tmux omitted {name}"))?
            .parse::<usize>()
            .map_err(|err| format!("invalid tmux {name}: {err}"))
    };
    let left = parse_dimension("pane_left", fields.next())?;
    let top = parse_dimension("pane_top", fields.next())?;
    let width = parse_dimension("pane_width", fields.next())?;
    let height = parse_dimension("pane_height", fields.next())?;
    if width == 0 || height == 0 {
        return Err("tmux returned an empty pane geometry".to_string());
    }
    Ok(TmuxPaneContext {
        path: PathBuf::from(path),
        geometry: TmuxPaneGeometry {
            left,
            top,
            width,
            height,
        },
    })
}

pub fn tmux_pane_context(tty_name: &str, tmux_executable: &str) -> Result<TmuxPaneContext, String> {
    let output = Command::new(tmux_executable)
        .args([
            "display-message",
            "-p",
            "-t",
            tty_name,
            "#{pane_current_path}\u{1f}#{pane_left}\u{1f}#{pane_top}\u{1f}#{pane_width}\u{1f}#{pane_height}",
        ])
        .output()
        .map_err(|err| format!("unable to query tmux: {err}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    parse_tmux_pane_context(&String::from_utf8_lossy(&output.stdout))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitFileStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Conflict,
}

impl GitFileStatus {
    fn from_porcelain(code: &[u8]) -> Option<Self> {
        if code == b"??" {
            return Some(Self::Untracked);
        }
        if code.len() != 2 || code == b"!!" {
            return None;
        }
        if code.contains(&b'U')
            || matches!(code, b"AA" | b"DD")
            || (code.contains(&b'A') && code.contains(&b'D'))
        {
            Some(Self::Conflict)
        } else if code.contains(&b'D') {
            Some(Self::Deleted)
        } else if code.contains(&b'R') || code.contains(&b'C') {
            Some(Self::Renamed)
        } else if code.contains(&b'A') {
            Some(Self::Added)
        } else if code.contains(&b'M') || code.contains(&b'T') {
            Some(Self::Modified)
        } else {
            None
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Modified => "M",
            Self::Added => "A",
            Self::Deleted => "D",
            Self::Renamed => "R",
            Self::Untracked => "U",
            Self::Conflict => "!",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitStatusSnapshot {
    pub requested_root: PathBuf,
    pub repository_root: Option<PathBuf>,
    pub statuses: HashMap<PathBuf, GitFileStatus>,
}

impl GitStatusSnapshot {
    pub fn read(requested_root: &Path) -> Self {
        let requested_root = normalize_existing_path(requested_root.to_path_buf());
        let repository_root = find_project_root(&requested_root);
        let mut snapshot = Self {
            requested_root,
            repository_root: repository_root.clone(),
            statuses: HashMap::new(),
        };
        let repository_root = match repository_root {
            Some(root) => root,
            None => return snapshot,
        };
        let output = match Command::new("git")
            .current_dir(&repository_root)
            .args(["status", "--porcelain=v1", "-z", "--untracked-files=all"])
            .output()
        {
            Ok(output) if output.status.success() => output,
            _ => return snapshot,
        };
        snapshot.statuses = parse_git_status(&repository_root, &output.stdout);
        snapshot
    }
}

fn parse_git_status(repository_root: &Path, porcelain: &[u8]) -> HashMap<PathBuf, GitFileStatus> {
    let records = porcelain
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .collect::<Vec<_>>();
    let mut statuses = HashMap::new();
    let mut index = 0;
    while index < records.len() {
        let record = records[index];
        if record.len() < 4 || record[2] != b' ' {
            index += 1;
            continue;
        }
        let status = GitFileStatus::from_porcelain(&record[..2]);
        let renamed = record[..2].contains(&b'R') || record[..2].contains(&b'C');
        let path = &record[3..];
        if let Some(status) = status {
            statuses.insert(
                repository_root.join(String::from_utf8_lossy(path).as_ref()),
                status,
            );
        }
        if renamed && index + 1 < records.len() {
            // Porcelain v1's -z form reports a rename as destination followed
            // by source; the destination is the path decorated in the tree.
            index += 1;
        }
        index += 1;
    }
    statuses
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexRateLimit {
    pub used_percent: u8,
    pub window_minutes: u64,
    pub resets_at: Option<u64>,
}

impl CodexRateLimit {
    pub fn window_label(&self) -> String {
        match self.window_minutes {
            300 => "5h".to_string(),
            1_440 => "day".to_string(),
            10_080 => "week".to_string(),
            43_200 | 44_640 => "month".to_string(),
            minutes if minutes % 10_080 == 0 => format!("{}w", minutes / 10_080),
            minutes if minutes % 1_440 == 0 => format!("{}d", minutes / 1_440),
            minutes if minutes % 60 == 0 => format!("{}h", minutes / 60),
            minutes => format!("{minutes}m"),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodexStatusSnapshot {
    pub active_loops: usize,
    pub primary: Option<CodexRateLimit>,
    pub secondary: Option<CodexRateLimit>,
    pub total_tokens: Option<u64>,
    pub source_updated_at: Option<u64>,
}

impl CodexStatusSnapshot {
    pub fn read(codex_home: &Path) -> Self {
        CodexStatusReader::default().read(codex_home)
    }

    pub fn usage_label(&self) -> String {
        let format_limit = |limit: &CodexRateLimit| {
            format!("{} {}% used", limit.window_label(), limit.used_percent)
        };
        match (&self.primary, &self.secondary) {
            (Some(primary), Some(secondary)) => {
                format!(
                    "Codex {} · {}",
                    format_limit(primary),
                    format_limit(secondary)
                )
            }
            (Some(primary), None) => format!("Codex {}", format_limit(primary)),
            (None, Some(secondary)) => format!("Codex {}", format_limit(secondary)),
            (None, None) => "Codex usage unavailable".to_string(),
        }
    }

    pub fn reset_label(&self, now: SystemTime) -> Option<String> {
        let reset = [
            self.primary.as_ref().and_then(|limit| limit.resets_at),
            self.secondary.as_ref().and_then(|limit| limit.resets_at),
        ]
        .into_iter()
        .flatten()
        .min()?;
        let now = now.duration_since(UNIX_EPOCH).ok()?.as_secs();
        let remaining = reset.saturating_sub(now);
        let label = if remaining == 0 {
            "reset due".to_string()
        } else if remaining < 60 * 60 {
            format!("resets in {}m", (remaining / 60).max(1))
        } else if remaining < 24 * 60 * 60 {
            format!("resets in {}h", (remaining / (60 * 60)).max(1))
        } else {
            format!("resets in {}d", (remaining / (24 * 60 * 60)).max(1))
        };
        Some(label)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SystemCapacitySnapshot {
    pub cpu_used_percent: Option<u8>,
    pub memory_used_bytes: Option<u64>,
    pub memory_total_bytes: Option<u64>,
}

impl SystemCapacitySnapshot {
    pub fn status_label(&self) -> Option<String> {
        let cpu = self
            .cpu_used_percent
            .map(|percent| format!("CPU {percent}%"));
        let memory = self
            .memory_used_bytes
            .zip(self.memory_total_bytes)
            .filter(|(_, total)| *total > 0)
            .map(|(used, total)| {
                format!(
                    "RAM {}/{} GB",
                    format_memory_gb(used.min(total)),
                    format_memory_gb(total)
                )
            });
        match (cpu, memory) {
            (Some(cpu), Some(memory)) => Some(format!("{cpu} · {memory}")),
            (Some(cpu), None) => Some(cpu),
            (None, Some(memory)) => Some(memory),
            (None, None) => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceStatusSnapshot {
    pub codex: CodexStatusSnapshot,
    pub system: SystemCapacitySnapshot,
}

fn format_memory_gb(bytes: u64) -> String {
    const GIB: u128 = 1024 * 1024 * 1024;
    let tenths = (u128::from(bytes) * 10 + GIB / 2) / GIB;
    if tenths % 10 == 0 {
        (tenths / 10).to_string()
    } else {
        format!("{}.{}", tenths / 10, tenths % 10)
    }
}

#[derive(Default)]
struct CodexStatusReader {
    observed_head: Option<(SystemTime, PathBuf)>,
    last_discovery: Option<Instant>,
    snapshot: CodexStatusSnapshot,
}

impl CodexStatusReader {
    fn read(&mut self, codex_home: &Path) -> CodexStatusSnapshot {
        self.snapshot.active_loops = codex_loop_count();
        let now = Instant::now();
        let mut discover = self
            .last_discovery
            .map(|last| now.duration_since(last) >= CODEX_ROLLOUT_DISCOVERY_INTERVAL)
            .unwrap_or(true);
        let mut rollouts = Vec::new();

        if !discover {
            if let Some((observed_modified, path)) = &self.observed_head {
                match fs::metadata(path).and_then(|metadata| metadata.modified()) {
                    Ok(modified) if modified != *observed_modified => {
                        rollouts.push((modified, path.clone()));
                    }
                    Ok(_) => return self.snapshot.clone(),
                    Err(_) => discover = true,
                }
            } else {
                return self.snapshot.clone();
            }
        }

        if discover {
            rollouts = newest_rollout_paths(&codex_home.join("sessions"));
            self.last_discovery = Some(now);
        }

        let head = rollouts.first().cloned();
        if head == self.observed_head {
            return self.snapshot.clone();
        }
        self.observed_head = head;
        for (modified, path) in rollouts.into_iter().take(CODEX_ROLLOUT_CANDIDATE_LIMIT) {
            if let Some((primary, secondary, total_tokens)) = read_latest_codex_token_count(&path) {
                self.snapshot.primary = primary;
                self.snapshot.secondary = secondary;
                self.snapshot.total_tokens = total_tokens;
                self.snapshot.source_updated_at = modified
                    .duration_since(UNIX_EPOCH)
                    .ok()
                    .map(|duration| duration.as_secs());
                break;
            }
        }
        self.snapshot.clone()
    }
}

#[derive(Default)]
struct WorkspaceStatusReader {
    codex: CodexStatusReader,
    system: SystemCapacityReader,
}

impl WorkspaceStatusReader {
    fn read(&mut self, codex_home: &Path) -> WorkspaceStatusSnapshot {
        WorkspaceStatusSnapshot {
            codex: self.codex.read(codex_home),
            system: self.system.read(),
        }
    }
}

struct SystemCapacityReader {
    #[cfg(target_os = "macos")]
    host: libc::host_t,
    #[cfg(target_os = "macos")]
    previous_cpu_ticks: Option<[u32; libc::CPU_STATE_MAX as usize]>,
    #[cfg(target_os = "macos")]
    memory_total_bytes: Option<u64>,
    #[cfg(target_os = "macos")]
    page_size_bytes: Option<u64>,
    #[cfg(target_os = "macos")]
    last_refresh: Option<Instant>,
    #[cfg(target_os = "macos")]
    snapshot: SystemCapacitySnapshot,
}

impl Default for SystemCapacityReader {
    fn default() -> Self {
        Self {
            #[cfg(target_os = "macos")]
            host: {
                #[allow(deprecated)]
                unsafe {
                    libc::mach_host_self()
                }
            },
            #[cfg(target_os = "macos")]
            previous_cpu_ticks: None,
            #[cfg(target_os = "macos")]
            memory_total_bytes: None,
            #[cfg(target_os = "macos")]
            page_size_bytes: None,
            #[cfg(target_os = "macos")]
            last_refresh: None,
            #[cfg(target_os = "macos")]
            snapshot: SystemCapacitySnapshot::default(),
        }
    }
}

impl SystemCapacityReader {
    fn read(&mut self) -> SystemCapacitySnapshot {
        #[cfg(target_os = "macos")]
        {
            let now = Instant::now();
            let needs_cpu_seed =
                self.snapshot.cpu_used_percent.is_none() && self.previous_cpu_ticks.is_some();
            if !needs_cpu_seed
                && self
                    .last_refresh
                    .map(|last| now.duration_since(last) < SYSTEM_CAPACITY_REFRESH_INTERVAL)
                    .unwrap_or(false)
            {
                return self.snapshot.clone();
            }

            let cpu_ticks = macos_cpu_ticks(self.host);
            let cpu_used_percent = self
                .previous_cpu_ticks
                .zip(cpu_ticks)
                .and_then(|(previous, current)| cpu_used_percent(previous, current));
            if cpu_ticks.is_some() {
                self.previous_cpu_ticks = cpu_ticks;
            }

            if self.memory_total_bytes.is_none() {
                self.memory_total_bytes = macos_total_memory_bytes();
            }
            if self.page_size_bytes.is_none() {
                let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
                if page_size > 0 {
                    self.page_size_bytes = Some(page_size as u64);
                }
            }
            let memory_used_bytes = self
                .page_size_bytes
                .and_then(|page_size| macos_memory_used_bytes(self.host, page_size))
                .map(|used| {
                    self.memory_total_bytes
                        .map(|total| used.min(total))
                        .unwrap_or(used)
                });

            self.snapshot = SystemCapacitySnapshot {
                cpu_used_percent,
                memory_used_bytes,
                memory_total_bytes: self.memory_total_bytes,
            };
            self.last_refresh = Some(now);
            self.snapshot.clone()
        }
        #[cfg(not(target_os = "macos"))]
        {
            SystemCapacitySnapshot::default()
        }
    }
}

fn cpu_used_percent<const N: usize>(previous: [u32; N], current: [u32; N]) -> Option<u8> {
    if N <= 2 {
        return None;
    }
    let deltas = std::array::from_fn::<u64, N, _>(|index| {
        u64::from(current[index].wrapping_sub(previous[index]))
    });
    let total = deltas.iter().copied().sum::<u64>();
    if total == 0 {
        return None;
    }
    let idle = deltas[2];
    let active = total.saturating_sub(idle);
    Some(((active * 100 + total / 2) / total).min(100) as u8)
}

#[cfg(target_os = "macos")]
fn macos_cpu_ticks(host: libc::host_t) -> Option<[u32; libc::CPU_STATE_MAX as usize]> {
    let mut ticks = [0_u32; libc::CPU_STATE_MAX as usize];
    let mut count = libc::HOST_CPU_LOAD_INFO_COUNT;
    // SAFETY: HOST_CPU_LOAD_INFO writes exactly CPU_STATE_MAX natural_t tick
    // counters. On Darwin natural_t is u32, matching this fixed-size buffer.
    let result = unsafe {
        libc::host_statistics(
            host,
            libc::HOST_CPU_LOAD_INFO,
            ticks.as_mut_ptr() as libc::host_info_t,
            &mut count,
        )
    };
    (result == libc::KERN_SUCCESS).then_some(ticks)
}

#[cfg(target_os = "macos")]
fn macos_total_memory_bytes() -> Option<u64> {
    let mut mib = [libc::CTL_HW, libc::HW_MEMSIZE];
    let mut total = 0_u64;
    let mut size = std::mem::size_of::<u64>();
    // SAFETY: `mib` requests the fixed-width hw.memsize value, and `total`
    // points to a writable u64 whose size is supplied to sysctl.
    let result = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as libc::c_uint,
            &mut total as *mut u64 as *mut libc::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    (result == 0 && size == std::mem::size_of::<u64>() && total > 0).then_some(total)
}

#[cfg(target_os = "macos")]
fn macos_memory_used_bytes(host: libc::host_t, page_size_bytes: u64) -> Option<u64> {
    let mut info = unsafe { std::mem::zeroed::<libc::vm_statistics64>() };
    let mut count = libc::HOST_VM_INFO64_COUNT;
    // SAFETY: `info` is the exact structure required for HOST_VM_INFO64,
    // and `count` advertises that structure's size in Mach integer units.
    let result = unsafe {
        libc::host_statistics64(
            host,
            libc::HOST_VM_INFO64,
            &mut info as *mut libc::vm_statistics64 as libc::host_info64_t,
            &mut count,
        )
    };
    if result != libc::KERN_SUCCESS {
        return None;
    }

    // Match macOS's meaningful occupied-memory view: anonymous pages that
    // cannot be purged, wired pages, and the compressed-memory store. This
    // excludes reclaimable file cache from the "used" number.
    let used_pages = u64::from(info.internal_page_count)
        .saturating_sub(u64::from(info.purgeable_count))
        .saturating_add(u64::from(info.wire_count))
        .saturating_add(u64::from(info.compressor_page_count));
    Some(used_pages.saturating_mul(page_size_bytes))
}

fn newest_rollout_paths(root: &Path) -> Vec<(SystemTime, PathBuf)> {
    let mut stack = vec![root.to_path_buf()];
    let mut rollouts = Vec::with_capacity(CODEX_ROLLOUT_CANDIDATE_LIMIT);
    while let Some(directory) = stack.pop() {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) => continue,
            };
            if file_type.is_dir() {
                stack.push(entry.path());
                continue;
            }
            let path = entry.path();
            if path.extension() != Some(OsStr::new("jsonl")) {
                continue;
            }
            let modified = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(UNIX_EPOCH);
            let candidate = (modified, path);
            if rollouts.len() < CODEX_ROLLOUT_CANDIDATE_LIMIT {
                rollouts.push(candidate);
            } else if let Some((oldest_index, oldest)) = rollouts
                .iter()
                .enumerate()
                .min_by_key(|(_, (modified, _))| *modified)
            {
                if candidate.0 > oldest.0 {
                    rollouts[oldest_index] = candidate;
                }
            }
        }
    }
    rollouts.sort_by(|left, right| right.0.cmp(&left.0));
    rollouts
}

fn read_latest_codex_token_count(
    path: &Path,
) -> Option<(Option<CodexRateLimit>, Option<CodexRateLimit>, Option<u64>)> {
    let mut file = File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let mut tail_bytes = CODEX_ROLLOUT_INITIAL_TAIL_BYTES;
    loop {
        let start = length.saturating_sub(tail_bytes);
        file.seek(SeekFrom::Start(start)).ok()?;
        let mut tail = Vec::with_capacity((length - start) as usize);
        file.read_to_end(&mut tail).ok()?;
        let text = String::from_utf8_lossy(&tail);
        if let Some(status) = text.lines().rev().find_map(parse_codex_token_count_line) {
            return Some(status);
        }
        if start == 0 || tail_bytes >= CODEX_ROLLOUT_MAX_TAIL_BYTES {
            return None;
        }
        tail_bytes = (tail_bytes * 2).min(CODEX_ROLLOUT_MAX_TAIL_BYTES);
    }
}

fn parse_codex_token_count_line(
    line: &str,
) -> Option<(Option<CodexRateLimit>, Option<CodexRateLimit>, Option<u64>)> {
    if !line.contains("\"type\":\"token_count\"") {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let payload = value.get("payload")?;
    if payload.get("type")?.as_str()? != "token_count" {
        return None;
    }
    let rate_limits = payload.get("rate_limits");
    let parse_limit = |name: &str| {
        let limit = rate_limits?.get(name)?;
        if limit.is_null() {
            return None;
        }
        let used_percent = limit.get("used_percent")?.as_f64()?.round().clamp(0., 100.) as u8;
        Some(CodexRateLimit {
            used_percent,
            window_minutes: limit.get("window_minutes")?.as_u64()?,
            resets_at: limit.get("resets_at").and_then(|value| value.as_u64()),
        })
    };
    let total_tokens = payload
        .pointer("/info/total_token_usage/total_tokens")
        .and_then(|value| value.as_u64());
    Some((
        parse_limit("primary"),
        parse_limit("secondary"),
        total_tokens,
    ))
}

fn is_codex_loop_executable(path: &Path) -> bool {
    path.file_name() == Some(OsStr::new("codex"))
}

#[cfg(target_os = "macos")]
fn codex_loop_count() -> usize {
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if count <= 0 {
        return 0;
    }
    let mut pids = vec![0 as libc::pid_t; count as usize + 32];
    let count = unsafe {
        libc::proc_listallpids(
            pids.as_mut_ptr() as *mut _,
            std::mem::size_of_val(pids.as_slice()) as _,
        )
    };
    if count <= 0 {
        return 0;
    }
    pids.truncate((count as usize).min(pids.len()));
    let mut buffer = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let mut active = 0;
    for pid in pids {
        let length =
            unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr() as *mut _, buffer.len() as u32) };
        if length <= 0 {
            continue;
        }
        if is_codex_loop_executable(Path::new(OsStr::from_bytes(&buffer[..length as usize]))) {
            active += 1;
        }
    }
    active
}

#[cfg(target_os = "macos")]
use std::os::unix::ffi::OsStrExt;

#[cfg(target_os = "linux")]
fn codex_loop_count() -> usize {
    fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| fs::read_link(entry.path().join("exe")).ok())
        .filter(|path| is_codex_loop_executable(path))
        .count()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn codex_loop_count() -> usize {
    0
}

#[derive(Debug, Clone)]
pub enum ServiceResponse {
    DirectoryListed(DirectoryListing),
    ContextResolved(PaneContext),
    GitStatusLoaded(GitStatusSnapshot),
    WorkspaceStatusLoaded(WorkspaceStatusSnapshot),
    FileSearchCompleted(FileSearchResult),
    FileLoaded(FileLoadResult),
    FileSaved(FileSaveResult),
    DirectoryChanged(Vec<PathBuf>),
    WatcherError(String),
}

enum ContextRequest {
    Pane(PaneContextRequest),
    WorkspaceStatus(PathBuf),
}

#[derive(Debug, Clone)]
pub enum FileRequest {
    Search {
        request_id: u64,
        root: PathBuf,
        query: String,
    },
    Load {
        request_id: u64,
        root: PathBuf,
        path: PathBuf,
    },
    Save {
        request_id: u64,
        root: PathBuf,
        path: PathBuf,
        content: String,
        expected_revision: Option<FileRevision>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileRevision {
    pub length: u64,
    pub modified_nanos: u128,
}

fn file_revision(metadata: &fs::Metadata) -> FileRevision {
    FileRevision {
        length: metadata.len(),
        modified_nanos: metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos())
            .unwrap_or(0),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSearchResult {
    pub request_id: u64,
    pub root: PathBuf,
    pub query: String,
    pub paths: Vec<PathBuf>,
    pub truncated: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileLoadResult {
    pub request_id: u64,
    pub path: PathBuf,
    pub content: Option<String>,
    pub revision: Option<FileRevision>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSaveResult {
    pub request_id: u64,
    pub path: PathBuf,
    pub revision: Option<FileRevision>,
    pub error: Option<String>,
}

fn canonical_workspace_path(root: &Path, path: &Path) -> io::Result<(PathBuf, PathBuf)> {
    let root = root.canonicalize()?;
    let path = path.canonicalize()?;
    if !path.starts_with(&root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "file is outside the active workspace",
        ));
    }
    Ok((root, path))
}

fn search_workspace_files(request_id: u64, root: PathBuf, query: String) -> FileSearchResult {
    let requested_root = root.clone();
    let canonical_root = match root.canonicalize() {
        Ok(root) => root,
        Err(error) => {
            return FileSearchResult {
                request_id,
                root,
                query,
                paths: vec![],
                truncated: false,
                error: Some(error.to_string()),
            };
        }
    };
    let needle = query.to_lowercase();
    let tokens = needle
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    let mut stack = vec![canonical_root.clone()];
    let mut candidates = Vec::new();
    let mut visited = 0usize;
    let mut truncated = false;
    while let Some(directory) = stack.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            visited += 1;
            if visited > MAX_FILE_SEARCH_ENTRIES {
                truncated = true;
                break;
            }
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) => continue,
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                let name = entry.file_name();
                if name != OsStr::new(".git") && name != OsStr::new("node_modules") {
                    stack.push(path);
                }
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            let relative = match path.strip_prefix(&canonical_root) {
                Ok(relative) => relative,
                Err(_) => continue,
            };
            let label = relative.to_string_lossy().to_lowercase();
            if tokens.iter().all(|token| label.contains(token)) {
                let first_match = tokens
                    .iter()
                    .filter_map(|token| label.find(token))
                    .min()
                    .unwrap_or(usize::MAX);
                candidates.push((
                    first_match,
                    label.len(),
                    relative.to_path_buf(),
                    requested_root.join(relative),
                ));
            }
        }
        if truncated {
            break;
        }
    }
    candidates.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.cmp(&right.2))
    });
    if candidates.len() > MAX_FILE_SEARCH_RESULTS {
        candidates.truncate(MAX_FILE_SEARCH_RESULTS);
        truncated = true;
    }
    FileSearchResult {
        request_id,
        root: requested_root,
        query,
        paths: candidates.into_iter().map(|(_, _, _, path)| path).collect(),
        truncated,
        error: None,
    }
}

fn load_workspace_file(request_id: u64, root: PathBuf, path: PathBuf) -> FileLoadResult {
    let result = (|| -> io::Result<(String, FileRevision)> {
        let (_, path) = canonical_workspace_path(&root, &path)?;
        let metadata = fs::metadata(&path)?;
        if !metadata.is_file() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a file"));
        }
        if metadata.len() > MAX_EDITABLE_FILE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "file is larger than the 2 MiB viewer limit",
            ));
        }
        let bytes = fs::read(path)?;
        let content = String::from_utf8(bytes)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "binary file"))?;
        Ok((content, file_revision(&metadata)))
    })();
    match result {
        Ok((content, revision)) => FileLoadResult {
            request_id,
            path,
            content: Some(content),
            revision: Some(revision),
            error: None,
        },
        Err(error) => FileLoadResult {
            request_id,
            path,
            content: None,
            revision: None,
            error: Some(error.to_string()),
        },
    }
}

fn save_workspace_file(
    request_id: u64,
    root: PathBuf,
    path: PathBuf,
    content: String,
    expected_revision: Option<FileRevision>,
) -> FileSaveResult {
    let result = (|| -> io::Result<FileRevision> {
        let (_, path) = canonical_workspace_path(&root, &path)?;
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "only regular workspace files can be saved",
            ));
        }
        if let Some(expected) = expected_revision {
            if file_revision(&metadata) != expected {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "file changed on disk; reopen it before saving",
                ));
            }
        }
        if content.len() as u64 > MAX_EDITABLE_FILE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "edited file exceeds the 2 MiB limit",
            ));
        }
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "file has no parent directory")
        })?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let temporary = parent.join(format!(".cosmos-save-{}-{nonce}", std::process::id()));
        let write_result = (|| -> io::Result<FileRevision> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.set_permissions(metadata.permissions())?;
            file.write_all(content.as_bytes())?;
            file.sync_all()?;
            fs::rename(&temporary, &path)?;
            Ok(file_revision(&fs::metadata(&path)?))
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        write_result
    })();
    FileSaveResult {
        request_id,
        path,
        revision: result.as_ref().ok().copied(),
        error: result.err().map(|error| error.to_string()),
    }
}

pub struct WorkspaceService {
    directory_tx: Sender<(PathBuf, bool)>,
    context_tx: Sender<ContextRequest>,
    git_status_tx: Sender<PathBuf>,
    file_tx: Sender<FileRequest>,
    watch_tx: Sender<HashSet<PathBuf>>,
    response_rx: Receiver<ServiceResponse>,
}

impl WorkspaceService {
    pub fn new() -> Self {
        let (directory_tx, directory_rx) = mpsc::channel::<(PathBuf, bool)>();
        let (context_tx, context_rx) = mpsc::channel::<ContextRequest>();
        let (git_status_tx, git_status_rx) = mpsc::channel::<PathBuf>();
        let (file_tx, file_rx) = mpsc::channel::<FileRequest>();
        let (watch_tx, watch_rx) = mpsc::channel::<HashSet<PathBuf>>();
        let (response_tx, response_rx) = mpsc::channel();
        let directory_response_tx = response_tx.clone();
        std::thread::Builder::new()
            .name("cosmos-directory-reader".to_string())
            .spawn(move || {
                while let Ok((path, show_hidden)) = directory_rx.recv() {
                    let response = ServiceResponse::DirectoryListed(DirectoryListing::read(
                        &path,
                        show_hidden,
                    ));
                    if directory_response_tx.send(response).is_err() {
                        break;
                    }
                }
            })
            .expect("spawn cosmos directory reader");

        let context_response_tx = response_tx.clone();
        std::thread::Builder::new()
            .name("cosmos-pane-context".to_string())
            .spawn(move || {
                let mut status_reader = WorkspaceStatusReader::default();
                while let Ok(request) = context_rx.recv() {
                    let response = match request {
                        ContextRequest::Pane(request) => {
                            ServiceResponse::ContextResolved(PaneContext::resolve(request))
                        }
                        ContextRequest::WorkspaceStatus(codex_home) => {
                            ServiceResponse::WorkspaceStatusLoaded(status_reader.read(&codex_home))
                        }
                    };
                    if context_response_tx.send(response).is_err() {
                        break;
                    }
                }
            })
            .expect("spawn cosmos pane context resolver");

        let git_response_tx = response_tx.clone();
        std::thread::Builder::new()
            .name("cosmos-git-status".to_string())
            .spawn(move || {
                while let Ok(root) = git_status_rx.recv() {
                    if git_response_tx
                        .send(ServiceResponse::GitStatusLoaded(GitStatusSnapshot::read(
                            &root,
                        )))
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .expect("spawn cosmos git status reader");

        let file_response_tx = response_tx.clone();
        std::thread::Builder::new()
            .name("cosmos-file-workspace".to_string())
            .spawn(move || {
                while let Ok(request) = file_rx.recv() {
                    let response = match request {
                        FileRequest::Search {
                            request_id,
                            root,
                            query,
                        } => ServiceResponse::FileSearchCompleted(search_workspace_files(
                            request_id, root, query,
                        )),
                        FileRequest::Load {
                            request_id,
                            root,
                            path,
                        } => {
                            ServiceResponse::FileLoaded(load_workspace_file(request_id, root, path))
                        }
                        FileRequest::Save {
                            request_id,
                            root,
                            path,
                            content,
                            expected_revision,
                        } => ServiceResponse::FileSaved(save_workspace_file(
                            request_id,
                            root,
                            path,
                            content,
                            expected_revision,
                        )),
                    };
                    if file_response_tx.send(response).is_err() {
                        break;
                    }
                }
            })
            .expect("spawn cosmos file workspace");

        std::thread::Builder::new()
            .name("cosmos-filesystem-watcher".to_string())
            .spawn(move || {
                use notify::Watcher;

                let event_tx = response_tx.clone();
                let mut watcher = match notify::recommended_watcher(
                    move |event: notify::Result<notify::Event>| {
                        let response = match event {
                            Ok(event) => ServiceResponse::DirectoryChanged(event.paths),
                            Err(err) => ServiceResponse::WatcherError(err.to_string()),
                        };
                        let _ = event_tx.send(response);
                    },
                ) {
                    Ok(watcher) => watcher,
                    Err(err) => {
                        let _ = response_tx.send(ServiceResponse::WatcherError(err.to_string()));
                        return;
                    }
                };
                let mut watched = HashSet::new();
                while let Ok(desired) = watch_rx.recv() {
                    for path in watched.difference(&desired) {
                        let _ = watcher.unwatch(path);
                    }
                    for path in desired.difference(&watched) {
                        if let Err(err) = watcher.watch(path, notify::RecursiveMode::NonRecursive) {
                            let _ = response_tx.send(ServiceResponse::WatcherError(format!(
                                "Unable to watch {}: {err}",
                                path.display()
                            )));
                        }
                    }
                    watched = desired;
                }
            })
            .expect("spawn cosmos filesystem watcher");

        Self {
            directory_tx,
            context_tx,
            git_status_tx,
            file_tx,
            watch_tx,
            response_rx,
        }
    }

    pub fn list_directory(&self, path: PathBuf, show_hidden: bool) {
        let _ = self.directory_tx.send((path, show_hidden));
    }

    pub fn resolve_context(&self, request: PaneContextRequest) {
        let _ = self.context_tx.send(ContextRequest::Pane(request));
    }

    pub fn workspace_status(&self, codex_home: PathBuf) {
        let _ = self
            .context_tx
            .send(ContextRequest::WorkspaceStatus(codex_home));
    }

    pub fn git_status(&self, root: PathBuf) {
        let _ = self.git_status_tx.send(root);
    }

    pub fn file_request(&self, request: FileRequest) {
        let _ = self.file_tx.send(request);
    }

    pub fn watch_directories(&self, paths: HashSet<PathBuf>) {
        let _ = self.watch_tx.send(paths);
    }

    pub fn try_recv(&self) -> Option<ServiceResponse> {
        self.response_rx.try_recv().ok()
    }
}

impl Default for WorkspaceService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn verifies_tmux_manager_close_lock_credentials() {
        let passphrase = "cosmos-test-passphrase";
        let salt = b"cosmos-test-salt";
        let iterations = NonZeroU32::new(10).unwrap();
        let mut digest = [0u8; 32];
        pbkdf2::derive(
            pbkdf2::PBKDF2_HMAC_SHA256,
            iterations,
            salt,
            passphrase.as_bytes(),
            &mut digest,
        );
        let credential = serde_json::to_vec(&serde_json::json!({
            "format_version": 1,
            "kdf": "pbkdf2-hmac-sha256",
            "iterations": iterations.get(),
            "salt": BASE64_STANDARD.encode(salt),
            "digest": BASE64_STANDARD.encode(digest),
        }))
        .unwrap();

        assert!(verify_close_lock_credential(&credential, passphrase).unwrap());
        assert!(!verify_close_lock_credential(&credential, "incorrect").unwrap());
        assert!(!verify_close_lock_credential(&credential, "").unwrap());
    }

    fn temporary_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("cosmos-workspace-{name}-{nonce}"))
    }

    #[test]
    fn follow_reveals_each_ancestor() {
        let root = PathBuf::from("/projects/cosmos");
        let target = root.join("src/ui/explorer");
        let mut state = ExplorerState::default();
        state.add_root(root.clone());
        state.reveal_path(&target, Some(&root));
        assert!(state.expanded.contains(&root));
        assert!(state.expanded.contains(&root.join("src")));
        assert!(state.expanded.contains(&root.join("src/ui")));
        assert!(state.expanded.contains(&target));
    }

    #[test]
    fn locked_mode_does_not_expand() {
        let root = PathBuf::from("/projects/cosmos");
        let target = root.join("src/ui");
        let mut state = ExplorerState {
            follow_mode: FollowMode::Locked,
            ..ExplorerState::default()
        };
        state.add_root(root);
        state.reveal_path(&target, None);
        assert!(state.expanded.is_empty());
    }

    #[test]
    fn longest_workspace_root_wins() {
        let mut state = ExplorerState::default();
        state.add_root(PathBuf::from("/projects"));
        state.add_root(PathBuf::from("/projects/cosmos"));
        assert_eq!(
            state.matching_root(Path::new("/projects/cosmos/src")),
            Some(1)
        );
    }

    #[test]
    fn state_round_trips() {
        let path = temporary_path("state").join("state.json");
        let mut state = ExplorerState::default();
        state.visible = false;
        state.width_px = 412;
        state.add_root(PathBuf::from("/projects/cosmos"));
        state.save(&path).unwrap();
        assert_eq!(ExplorerState::load(&path).unwrap(), state);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn legacy_state_migrates_to_vscode_layout_defaults() {
        let path = temporary_path("legacy-layout").join("state.json");
        let mut state = ExplorerState::default();
        state.layout_version = 0;
        state.visible = false;
        state.width_px = 300;
        state.show_hidden = false;
        state.follow_mode = FollowMode::Locked;
        state.save(&path).unwrap();

        let migrated = ExplorerState::load(&path).unwrap();
        assert_eq!(migrated.layout_version, CURRENT_LAYOUT_VERSION);
        assert!(migrated.visible);
        assert_eq!(migrated.width_px, DEFAULT_SIDEBAR_WIDTH);
        assert!(migrated.show_hidden);
        assert_eq!(migrated.follow_mode, FollowMode::Follow);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn prior_layout_migrates_to_roomier_sidebar() {
        let path = temporary_path("roomier-sidebar").join("state.json");
        let mut state = ExplorerState::default();
        state.layout_version = CURRENT_LAYOUT_VERSION - 1;
        state.width_px = 355;
        state.save(&path).unwrap();

        let migrated = ExplorerState::load(&path).unwrap();
        assert_eq!(migrated.layout_version, CURRENT_LAYOUT_VERSION);
        assert_eq!(migrated.width_px, DEFAULT_SIDEBAR_WIDTH);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn parses_git_porcelain_decorations() {
        let root = Path::new("/projects/cosmos");
        let statuses = parse_git_status(
            root,
            b" M src/serviceWorker.js\0?? src/new file.js\0R  src/current.js\0src/old.js\0",
        );

        assert_eq!(
            statuses.get(&root.join("src/serviceWorker.js")),
            Some(&GitFileStatus::Modified)
        );
        assert_eq!(
            statuses.get(&root.join("src/new file.js")),
            Some(&GitFileStatus::Untracked)
        );
        assert_eq!(
            statuses.get(&root.join("src/current.js")),
            Some(&GitFileStatus::Renamed)
        );
        assert!(!statuses.contains_key(&root.join("src/old.js")));
    }

    #[test]
    fn directory_listing_is_lazy_and_sorted() {
        let root = temporary_path("listing");
        fs::create_dir_all(root.join("z-dir")).unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("a-file"), b"hello").unwrap();
        fs::write(root.join(".hidden"), b"secret").unwrap();
        let listing = DirectoryListing::read(&root, false);
        assert_eq!(listing.entries.len(), 2);
        assert!(listing.entries[0].is_dir);
        assert_eq!(listing.entries[0].name, "z-dir");
        assert_eq!(listing.entries[1].name, "a-file");
        let visible_hidden = DirectoryListing::read(&root, true);
        assert!(visible_hidden
            .entries
            .iter()
            .any(|entry| entry.name == ".hidden"));
        assert!(!visible_hidden
            .entries
            .iter()
            .any(|entry| entry.name == ".git"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn scoped_rows_only_include_the_visible_root() {
        let visible = temporary_path("visible-root");
        let sibling = temporary_path("sibling-root");
        fs::create_dir_all(visible.join("inside")).unwrap();
        fs::create_dir_all(sibling.join("outside")).unwrap();

        let mut state = ExplorerState::default();
        state.add_root(visible.clone());
        state.add_root(sibling.clone());
        state.expanded.insert(visible.clone());
        state.expanded.insert(sibling.clone());

        let mut cache = DirectoryCache::default();
        cache.apply(DirectoryListing::read(&visible, false));
        cache.apply(DirectoryListing::read(&sibling, false));
        let scoped = WorkspaceRoot::new(visible.clone());
        let rows = cache.rows_for_root(&state, &scoped, usize::MAX);

        assert!(rows.iter().any(|row| row.label == "inside"));
        assert!(!rows.iter().any(|row| row.label == "outside"));

        let _ = fs::remove_dir_all(visible);
        let _ = fs::remove_dir_all(sibling);
    }

    #[test]
    fn scoped_rows_do_not_show_parent_siblings() {
        let parent = temporary_path("scoped-parent");
        let visible = parent.join("cosmos");
        fs::create_dir_all(visible.join("src")).unwrap();
        fs::create_dir_all(parent.join("unrelated")).unwrap();

        let mut state = ExplorerState::default();
        state.add_root(parent.clone());
        state.expanded.insert(parent.clone());
        state.expanded.insert(visible.clone());

        let mut cache = DirectoryCache::default();
        cache.apply(DirectoryListing::read(&parent, false));
        cache.apply(DirectoryListing::read(&visible, false));
        let rows = cache.rows_for_root(&state, &WorkspaceRoot::new(visible.clone()), usize::MAX);

        assert_eq!(rows[0].path.as_deref(), Some(visible.as_path()));
        assert!(rows.iter().any(|row| row.label == "src"));
        assert!(!rows.iter().any(|row| row.label == "unrelated"));

        let _ = fs::remove_dir_all(parent);
    }

    #[test]
    fn detects_tmux_executable_by_basename() {
        assert!(process_is_tmux(Some("/opt/homebrew/bin/tmux")));
        assert!(process_is_tmux(Some("tmux")));
        assert!(!process_is_tmux(Some("/bin/zsh")));
        assert!(!process_is_tmux(None));
    }

    #[test]
    fn parses_tmux_path_and_active_pane_geometry() {
        let context =
            parse_tmux_pane_context("/tmp/cosmos project\u{1f}41\u{1f}3\u{1f}80\u{1f}24\n")
                .unwrap();
        assert_eq!(context.path, PathBuf::from("/tmp/cosmos project"));
        assert_eq!(
            context.geometry,
            TmuxPaneGeometry {
                left: 41,
                top: 3,
                width: 80,
                height: 24,
            }
        );
        assert!(parse_tmux_pane_context("/tmp\u{1f}0\u{1f}0\u{1f}0\u{1f}24").is_err());
    }

    #[test]
    fn parses_codex_usage_without_reading_transcript_content() {
        let line = r#"{"timestamp":"2026-07-17T06:46:16.167Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"total_tokens":217947530}},"rate_limits":{"primary":{"used_percent":48.4,"window_minutes":10080,"resets_at":1784780148},"secondary":{"used_percent":12.0,"window_minutes":300,"resets_at":1784300000}}}}"#;
        let (primary, secondary, total_tokens) = parse_codex_token_count_line(line).unwrap();
        assert_eq!(
            primary,
            Some(CodexRateLimit {
                used_percent: 48,
                window_minutes: 10_080,
                resets_at: Some(1_784_780_148),
            })
        );
        assert_eq!(
            secondary,
            Some(CodexRateLimit {
                used_percent: 12,
                window_minutes: 300,
                resets_at: Some(1_784_300_000),
            })
        );
        assert_eq!(total_tokens, Some(217_947_530));
    }

    #[test]
    fn codex_rollout_discovery_keeps_only_bounded_candidates() {
        let root = temporary_path("codex-rollouts");
        fs::create_dir_all(&root).unwrap();
        for index in 0..(CODEX_ROLLOUT_CANDIDATE_LIMIT + 9) {
            fs::write(root.join(format!("rollout-{index:02}.jsonl")), b"{}\n").unwrap();
        }

        let rollouts = newest_rollout_paths(&root);
        assert_eq!(rollouts.len(), CODEX_ROLLOUT_CANDIDATE_LIMIT);
        assert!(rollouts.windows(2).all(|pair| pair[0].0 >= pair[1].0));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn formats_compact_codex_status_labels() {
        let status = CodexStatusSnapshot {
            active_loops: 5,
            primary: Some(CodexRateLimit {
                used_percent: 48,
                window_minutes: 10_080,
                resets_at: None,
            }),
            secondary: None,
            total_tokens: None,
            source_updated_at: None,
        };
        assert_eq!(status.usage_label(), "Codex week 48% used");
        assert_eq!(
            CodexRateLimit {
                used_percent: 1,
                window_minutes: 300,
                resets_at: None,
            }
            .window_label(),
            "5h"
        );
    }

    #[test]
    fn calculates_cpu_usage_from_tick_deltas() {
        assert_eq!(
            cpu_used_percent([100, 50, 700, 10], [130, 70, 750, 10]),
            Some(50)
        );
        assert_eq!(cpu_used_percent([1, 2, 3, 4], [1, 2, 3, 4]), None);
    }

    #[test]
    fn formats_compact_system_capacity_labels() {
        let gib = 1024_u64 * 1024 * 1024;
        let capacity = SystemCapacitySnapshot {
            cpu_used_percent: Some(12),
            memory_used_bytes: Some(18 * gib + gib / 2),
            memory_total_bytes: Some(36 * gib),
        };
        assert_eq!(
            capacity.status_label().as_deref(),
            Some("CPU 12% · RAM 18.5/36 GB")
        );
        assert_eq!(
            SystemCapacitySnapshot {
                memory_used_bytes: Some(8 * gib),
                memory_total_bytes: Some(16 * gib),
                ..SystemCapacitySnapshot::default()
            }
            .status_label()
            .as_deref(),
            Some("RAM 8/16 GB")
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn reads_native_macos_system_capacity() {
        let mut reader = SystemCapacityReader::default();
        let snapshot = reader.read();
        let used = snapshot.memory_used_bytes.unwrap();
        let total = snapshot.memory_total_bytes.unwrap();
        assert!(used > 0);
        assert!(used <= total);
        assert!(macos_cpu_ticks(reader.host).is_some());
    }

    #[test]
    fn shows_the_next_codex_reset() {
        let now = UNIX_EPOCH + Duration::from_secs(1_000);
        let status = CodexStatusSnapshot {
            primary: Some(CodexRateLimit {
                used_percent: 48,
                window_minutes: 10_080,
                resets_at: Some(1_000 + 3 * 24 * 60 * 60),
            }),
            secondary: Some(CodexRateLimit {
                used_percent: 12,
                window_minutes: 300,
                resets_at: Some(1_000 + 2 * 60 * 60),
            }),
            ..CodexStatusSnapshot::default()
        };
        assert_eq!(status.reset_label(now).as_deref(), Some("resets in 2h"));
    }

    #[test]
    fn counts_only_the_codex_root_executable_name() {
        assert!(is_codex_loop_executable(Path::new(
            "/Users/example/.local/bin/codex"
        )));
        assert!(!is_codex_loop_executable(Path::new(
            "/Users/example/.local/bin/codex-code-mode-host"
        )));
        assert!(!is_codex_loop_executable(Path::new("/usr/bin/node")));
    }

    #[test]
    fn searches_loads_and_atomically_saves_workspace_files() {
        let root = temporary_path("file-workspace");
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("README.md"), "# Cosmos\n").unwrap();
        fs::write(root.join("docs/guide.md"), "before\n").unwrap();
        fs::write(root.join(".git/hidden.md"), "hidden\n").unwrap();

        let search = search_workspace_files(7, root.clone(), "guide".to_string());
        assert_eq!(search.request_id, 7);
        assert_eq!(search.paths, vec![root.join("docs/guide.md")]);
        assert!(!search.truncated);
        assert!(search.error.is_none());

        let loaded = load_workspace_file(8, root.clone(), root.join("README.md"));
        assert_eq!(loaded.content.as_deref(), Some("# Cosmos\n"));
        assert!(loaded.revision.is_some());
        assert!(loaded.error.is_none());

        let guide = load_workspace_file(9, root.clone(), root.join("docs/guide.md"));
        let saved = save_workspace_file(
            10,
            root.clone(),
            root.join("docs/guide.md"),
            "after\n".to_string(),
            guide.revision,
        );
        assert!(saved.error.is_none());
        assert!(saved.revision.is_some());
        assert_eq!(
            fs::read_to_string(root.join("docs/guide.md")).unwrap(),
            "after\n"
        );
        assert!(fs::read_dir(root.join("docs"))
            .unwrap()
            .flatten()
            .all(|entry| !entry
                .file_name()
                .to_string_lossy()
                .starts_with(".cosmos-save-")));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn file_workspace_rejects_paths_outside_the_active_root() {
        let root = temporary_path("file-root");
        let outside = temporary_path("file-outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("private.md"), "do not read\n").unwrap();

        let loaded = load_workspace_file(1, root.clone(), outside.join("private.md"));
        assert!(loaded.content.is_none());
        assert!(loaded
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("outside"));

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }

    #[test]
    fn file_workspace_refuses_to_overwrite_external_changes() {
        let root = temporary_path("file-conflict");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("notes.md");
        fs::write(&path, "one\n").unwrap();
        let loaded = load_workspace_file(1, root.clone(), path.clone());
        fs::write(&path, "external change with a different length\n").unwrap();

        let saved = save_workspace_file(
            2,
            root.clone(),
            path.clone(),
            "editor change\n".to_string(),
            loaded.revision,
        );
        assert!(saved
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("changed on disk"));
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "external change with a different length\n"
        );

        let _ = fs::remove_dir_all(root);
    }
}
