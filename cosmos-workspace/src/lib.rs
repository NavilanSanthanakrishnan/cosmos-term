use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::SystemTime;

pub const DEFAULT_SIDEBAR_WIDTH: usize = 300;
pub const MIN_SIDEBAR_WIDTH: usize = 190;
pub const MAX_SIDEBAR_WIDTH: usize = 720;
pub const MAX_DIRECTORY_ENTRIES: usize = 5_000;

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
            visible: true,
            width_px: DEFAULT_SIDEBAR_WIDTH,
            roots: vec![],
            expanded: BTreeSet::new(),
            follow_mode: FollowMode::Follow,
            show_hidden: false,
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
    pub error: Option<String>,
}

impl PaneContext {
    pub fn resolve(request: PaneContextRequest) -> Self {
        let mut cwd = None;
        let mut source = ContextSource::Unknown;
        let mut error = None;

        if process_is_tmux(request.foreground_process.as_deref()) {
            if let Some(tty_name) = request.tty_name.as_deref() {
                match tmux_current_path(
                    tty_name,
                    request.foreground_process.as_deref().unwrap_or("tmux"),
                ) {
                    Ok(path) => {
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

pub fn tmux_current_path(tty_name: &str, tmux_executable: &str) -> Result<PathBuf, String> {
    let output = Command::new(tmux_executable)
        .args([
            "display-message",
            "-p",
            "-t",
            tty_name,
            "#{pane_current_path}",
        ])
        .output()
        .map_err(|err| format!("unable to query tmux: {err}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        return Err("tmux returned an empty pane path".to_string());
    }
    Ok(PathBuf::from(path))
}

#[derive(Debug, Clone)]
pub enum ServiceResponse {
    DirectoryListed(DirectoryListing),
    ContextResolved(PaneContext),
    DirectoryChanged(Vec<PathBuf>),
    WatcherError(String),
}

pub struct WorkspaceService {
    directory_tx: Sender<(PathBuf, bool)>,
    context_tx: Sender<PaneContextRequest>,
    watch_tx: Sender<HashSet<PathBuf>>,
    response_rx: Receiver<ServiceResponse>,
}

impl WorkspaceService {
    pub fn new() -> Self {
        let (directory_tx, directory_rx) = mpsc::channel::<(PathBuf, bool)>();
        let (context_tx, context_rx) = mpsc::channel::<PaneContextRequest>();
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
                while let Ok(request) = context_rx.recv() {
                    if context_response_tx
                        .send(ServiceResponse::ContextResolved(PaneContext::resolve(
                            request,
                        )))
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .expect("spawn cosmos pane context resolver");

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
            watch_tx,
            response_rx,
        }
    }

    pub fn list_directory(&self, path: PathBuf, show_hidden: bool) {
        let _ = self.directory_tx.send((path, show_hidden));
    }

    pub fn resolve_context(&self, request: PaneContextRequest) {
        let _ = self.context_tx.send(request);
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
    fn directory_listing_is_lazy_and_sorted() {
        let root = temporary_path("listing");
        fs::create_dir_all(root.join("z-dir")).unwrap();
        fs::write(root.join("a-file"), b"hello").unwrap();
        fs::write(root.join(".hidden"), b"secret").unwrap();
        let listing = DirectoryListing::read(&root, false);
        assert_eq!(listing.entries.len(), 2);
        assert!(listing.entries[0].is_dir);
        assert_eq!(listing.entries[0].name, "z-dir");
        assert_eq!(listing.entries[1].name, "a-file");
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
}
