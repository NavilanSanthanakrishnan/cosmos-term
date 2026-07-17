use super::{TermWindow, TermWindowNotif, UIItem, UIItemType};
use crate::customglyph::{BlockAlpha, BlockCoord, Poly, PolyCommand, PolyStyle};
use crate::quad::{QuadTrait, TripleLayerQuadAllocator, TripleLayerQuadAllocatorTrait};
use crate::spawn::SpawnWhere;
use crate::termwindow::box_model::{Element, ElementCell, ElementContent, LayoutContext};
use crate::utilsprites::RenderMetrics;
use ::window::{
    KeyCode, Modifiers, MouseCursor, MouseEvent, MouseEventKind, MousePress, Window, WindowOps,
};
use config::keyassignment::{SpawnCommand, SpawnTabDomain};
use config::{DimensionContext, FontAttributes, FontWeight, TextStyle};
use cosmos_workspace::{
    expand_home, CodexStatusSnapshot, DirectoryCache, ExplorerRow, ExplorerRowKind, ExplorerState,
    FollowMode, GitFileStatus, PaneContext, PaneContextRequest, ServiceResponse, WorkspaceRoot,
    WorkspaceService, MAX_SIDEBAR_WIDTH, MIN_SIDEBAR_WIDTH,
};
use mux::pane::CachePolicy;
use mux::tab::{SplitDirection, SplitRequest, SplitSize};
use mux::Mux;
use ordered_float::NotNan;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};
use termwiz::cell::CellAttributes;
use termwiz::color::RgbColor;
use termwiz::input::{InputEvent, KeyCode as TermKeyCode, KeyEvent as TermKeyEvent};
use termwiz::lineedit::{Action, BasicHistory, History, LineEditor, LineEditorHost};
use termwiz::surface::Change;
use termwiz::terminal::Terminal;
use unicode_width::UnicodeWidthStr;

// VS Code defines these values in logical CSS pixels. Convert the complete
// explorer layout to physical pixels at the window DPI so its apparent size
// remains stable across standard and Retina displays.
const DIVIDER_WIDTH: usize = 1;
const DIVIDER_HIT_WIDTH: usize = 7;
const TITLE_HEIGHT: usize = 35;
const ROW_HEIGHT: usize = 22;
const TITLE_LEFT: usize = 20;
const TREE_LEFT: usize = 4;
const TREE_INDENT: usize = 8;
const ICON_SIZE: usize = 16;
const ACTION_SIZE: usize = 22;
const SCROLLBAR_SIZE: usize = 10;
const STATUS_BAR_HEIGHT: usize = 22;
const EXPLORER_HEADER_FONT_LOGICAL_SIZE: f64 = 13.0;
const EXPLORER_BODY_FONT_LOGICAL_SIZE: f64 = 15.0;
const EXPLORER_ICON_FONT_LOGICAL_SIZE: f64 = 16.0;
const STATUS_BAR_FONT_LOGICAL_SIZE: f64 = 12.0;
#[cfg(target_os = "macos")]
const EXPLORER_UI_FONT_FAMILY: &str = "System Font";
#[cfg(not(target_os = "macos"))]
const EXPLORER_UI_FONT_FAMILY: &str = "Helvetica Neue";
const VIRTUAL_ROOT_INDEX: usize = usize::MAX;
const SERVICE_POLL_INTERVAL: Duration = Duration::from_millis(50);
const CONTEXT_POLL_INTERVAL: Duration = Duration::from_millis(250);
const DIRECTORY_REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const CODEX_STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

fn logical_to_physical(value: usize, dpi: usize) -> usize {
    if value == 0 {
        return 0;
    }
    ((value as f64 * dpi.max(1) as f64 / 72.).round() as usize).max(1)
}

fn physical_to_logical(value: usize, dpi: usize) -> usize {
    ((value as f64 * 72. / dpi.max(1) as f64).round() as usize).max(1)
}

const SIDEBAR_BG: RgbColor = RgbColor::new_8bpc(37, 37, 38);
const DIVIDER: RgbColor = RgbColor::new_8bpc(43, 43, 43);
const TEXT: RgbColor = RgbColor::new_8bpc(204, 204, 204);
const MUTED: RgbColor = RgbColor::new_8bpc(157, 157, 157);
const INDENT_GUIDE: RgbColor = RgbColor::new_8bpc(50, 50, 50);
const HOVER_BG: RgbColor = RgbColor::new_8bpc(42, 45, 46);
const INACTIVE_SELECTION_BG: RgbColor = RgbColor::new_8bpc(55, 55, 61);
const ACTIVE_SELECTION_BG: RgbColor = RgbColor::new_8bpc(4, 57, 94);
// VS Code's dark scrollbar is #797979 at 40% over the #252526 sidebar.
const SCROLLBAR: RgbColor = RgbColor::new_8bpc(71, 71, 71);
const ERROR: RgbColor = RgbColor::new_8bpc(248, 128, 112);
// VS Code Dark Modern's status bar tokens.
const STATUS_BAR_BG: RgbColor = RgbColor::new_8bpc(24, 24, 24);
const STATUS_BAR_BORDER: RgbColor = RgbColor::new_8bpc(43, 43, 43);
const STATUS_BAR_TEXT: RgbColor = RgbColor::new_8bpc(204, 204, 204);
const STATUS_BAR_LIVE: RgbColor = RgbColor::new_8bpc(137, 209, 133);

const CHEVRON_RIGHT: &[Poly] = &[Poly {
    path: &[
        PolyCommand::MoveTo(BlockCoord::Frac(3, 8), BlockCoord::Frac(2, 8)),
        PolyCommand::LineTo(BlockCoord::Frac(5, 8), BlockCoord::Frac(4, 8)),
        PolyCommand::LineTo(BlockCoord::Frac(3, 8), BlockCoord::Frac(6, 8)),
    ],
    intensity: BlockAlpha::Full,
    style: PolyStyle::OutlineThin,
}];

const CHEVRON_DOWN: &[Poly] = &[Poly {
    path: &[
        PolyCommand::MoveTo(BlockCoord::Frac(2, 8), BlockCoord::Frac(3, 8)),
        PolyCommand::LineTo(BlockCoord::Frac(4, 8), BlockCoord::Frac(5, 8)),
        PolyCommand::LineTo(BlockCoord::Frac(6, 8), BlockCoord::Frac(3, 8)),
    ],
    intensity: BlockAlpha::Full,
    style: PolyStyle::OutlineThin,
}];

fn explorer_file_icon(path: &Path) -> (&'static str, RgbColor) {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    if name == "yarn.lock" || name.starts_with("yarn.") {
        return ("\u{e0a6}", RgbColor::new_8bpc(81, 154, 186));
    }
    if name.starts_with("readme") {
        return ("\u{e04d}", RgbColor::new_8bpc(81, 154, 186));
    }
    if name.starts_with(".git") {
        return ("\u{e034}", RgbColor::new_8bpc(65, 83, 91));
    }
    if name.starts_with("license") || name.starts_with("licence") || name.starts_with("copying") {
        return ("\u{e05a}", RgbColor::new_8bpc(203, 203, 65));
    }
    if name == "makefile" || name == "gnumakefile" {
        return ("\u{e05f}", RgbColor::new_8bpc(227, 121, 51));
    }
    if name == "dockerfile" || name.starts_with("dockerfile.") {
        return ("\u{e025}", RgbColor::new_8bpc(81, 154, 186));
    }

    match extension.as_str() {
        "js" | "cjs" | "mjs" => ("\u{e051}", RgbColor::new_8bpc(203, 203, 65)),
        "jsx" | "tsx" => ("\u{e07d}", RgbColor::new_8bpc(81, 154, 186)),
        "ts" => ("\u{e099}", RgbColor::new_8bpc(81, 154, 186)),
        "css" | "less" => ("\u{e01d}", RgbColor::new_8bpc(81, 154, 186)),
        "scss" | "sass" => ("\u{e084}", RgbColor::new_8bpc(245, 83, 133)),
        "json" | "jsonc" => ("\u{e055}", RgbColor::new_8bpc(203, 203, 65)),
        "svg" => ("\u{e091}", RgbColor::new_8bpc(160, 116, 196)),
        "md" | "mdx" | "markdown" => ("\u{e060}", RgbColor::new_8bpc(81, 154, 186)),
        "html" | "htm" => ("\u{e048}", RgbColor::new_8bpc(227, 121, 51)),
        "xml" => ("\u{e0a5}", RgbColor::new_8bpc(227, 121, 51)),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "ico" => {
            ("\u{e04c}", RgbColor::new_8bpc(160, 116, 196))
        }
        "rs" => ("\u{e082}", RgbColor::new_8bpc(109, 128, 134)),
        "py" => ("\u{e07b}", RgbColor::new_8bpc(81, 154, 186)),
        "sh" | "zsh" | "bash" => ("\u{e089}", RgbColor::new_8bpc(141, 193, 73)),
        "toml" | "ini" | "conf" | "config" => ("\u{e019}", RgbColor::new_8bpc(109, 128, 134)),
        "lock" | "key" | "pem" | "crt" | "cert" => ("\u{e05d}", RgbColor::new_8bpc(141, 193, 73)),
        "ttf" | "otf" | "woff" | "woff2" => ("\u{e033}", RgbColor::new_8bpc(204, 62, 68)),
        _ => ("\u{e023}", RgbColor::new_8bpc(212, 215, 214)),
    }
}

fn git_status_color(status: GitFileStatus) -> RgbColor {
    match status {
        GitFileStatus::Modified | GitFileStatus::Renamed => RgbColor::new_8bpc(226, 192, 141),
        GitFileStatus::Added | GitFileStatus::Untracked => RgbColor::new_8bpc(115, 201, 145),
        GitFileStatus::Deleted => RgbColor::new_8bpc(199, 78, 57),
        GitFileStatus::Conflict => RgbColor::new_8bpc(228, 103, 107),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerHeaderAction {
    RevealActive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerUiItem {
    Surface,
    Divider,
    Header(ExplorerHeaderAction),
    Row(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerPromptKind {
    AddRoot,
    RenameRoot(usize),
}

pub struct ExplorerUi {
    pub state: ExplorerState,
    state_path: PathBuf,
    service: WorkspaceService,
    cache: DirectoryCache,
    pending_directories: HashSet<PathBuf>,
    watched_directories: HashSet<PathBuf>,
    rows: Vec<ExplorerRow>,
    rendered_start: usize,
    rendered_capacity: usize,
    selected_path: Option<PathBuf>,
    pub(super) selected_root: Option<usize>,
    active_context: Option<PaneContext>,
    display_root: Option<WorkspaceRoot>,
    focused: bool,
    scroll: usize,
    context_request_in_flight: bool,
    git_statuses: HashMap<PathBuf, GitFileStatus>,
    git_status_request_in_flight: bool,
    codex_home: PathBuf,
    codex_status: CodexStatusSnapshot,
    codex_status_request_in_flight: bool,
    last_context_request: Instant,
    last_directory_refresh: Instant,
    last_git_status_refresh: Instant,
    last_codex_status_refresh: Instant,
    tick_scheduled: bool,
    last_error: Option<String>,
}

impl ExplorerUi {
    pub fn load() -> Self {
        let state_path = config::DATA_DIR.join("workspace-state.json");
        let (mut state, mut last_error, state_loaded) = match ExplorerState::load(&state_path) {
            Ok(state) => (state, None, true),
            Err(err) => (
                ExplorerState::default(),
                Some(format!("Unable to load explorer state: {err}")),
                false,
            ),
        };
        // The explorer is a permanent part of the Cosmos workbench. Keep the
        // serialized field for state compatibility, but never restore hidden.
        state.visible = true;
        // Always present the active pane's directory as the single visible
        // workspace root. Historical roots remain serialized for compatibility
        // but must not pin the tree to an unrelated directory.
        state.follow_mode = FollowMode::Follow;
        // Wait for the active pane context instead of flashing a stale saved
        // root from an unrelated prior shell directory.
        let display_root = None;
        // Persist layout migrations and the permanent-view invariants now, so
        // they do not depend on a later context change or divider drag.
        if state_loaded {
            if let Err(err) = state.save(&state_path) {
                last_error = Some(format!("Unable to save explorer state: {err}"));
            }
        }
        let codex_home = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| config::HOME_DIR.join(".codex"));
        Self {
            state,
            state_path,
            service: WorkspaceService::new(),
            cache: DirectoryCache::default(),
            pending_directories: HashSet::new(),
            watched_directories: HashSet::new(),
            rows: vec![],
            rendered_start: 0,
            rendered_capacity: 0,
            selected_path: None,
            selected_root: None,
            active_context: None,
            display_root,
            focused: false,
            scroll: 0,
            context_request_in_flight: false,
            git_statuses: HashMap::new(),
            git_status_request_in_flight: false,
            codex_home,
            codex_status: CodexStatusSnapshot::default(),
            codex_status_request_in_flight: false,
            last_context_request: Instant::now()
                .checked_sub(CONTEXT_POLL_INTERVAL)
                .unwrap_or_else(Instant::now),
            last_directory_refresh: Instant::now(),
            last_git_status_refresh: Instant::now()
                .checked_sub(DIRECTORY_REFRESH_INTERVAL)
                .unwrap_or_else(Instant::now),
            last_codex_status_refresh: Instant::now()
                .checked_sub(CODEX_STATUS_REFRESH_INTERVAL)
                .unwrap_or_else(Instant::now),
            tick_scheduled: false,
            last_error,
        }
    }

    pub fn total_width(&self, dpi: usize) -> usize {
        logical_to_physical(self.state.width_px + DIVIDER_WIDTH, dpi)
    }

    fn persist(&mut self) {
        if let Err(err) = self.state.save(&self.state_path) {
            self.last_error = Some(format!("Unable to save explorer state: {err}"));
        }
    }

    fn request_directory(&mut self, path: PathBuf, refresh: bool) {
        if !refresh && self.cache.is_loaded(&path) {
            return;
        }
        if self.pending_directories.insert(path.clone()) {
            self.cache.mark_loading(path.clone());
            self.service.list_directory(path, self.state.show_hidden);
        }
    }

    fn sync_expanded_directories(&mut self, refresh: bool) {
        let root = match self.display_root.as_ref() {
            Some(root) => root,
            None => {
                if !self.watched_directories.is_empty() {
                    self.watched_directories.clear();
                    self.service.watch_directories(HashSet::new());
                }
                return;
            }
        };
        let root_index = self
            .exact_root_index(&root.path)
            .unwrap_or(VIRTUAL_ROOT_INDEX);
        let mut ordered = vec![root.path.clone()];
        // Only queue expanded directories reachable through the currently
        // loaded tree. This preserves lazy loading and prevents stale deep
        // expansions (including offline folders) from running before the cwd.
        for row in self.cache.rows_for_root(&self.state, root, root_index) {
            if row.expanded
                && matches!(row.kind, ExplorerRowKind::Root | ExplorerRowKind::Directory)
            {
                if let Some(path) = row.path {
                    if path != root.path {
                        ordered.push(path);
                    }
                }
            }
        }
        ordered.sort_by(|left, right| {
            left.components()
                .count()
                .cmp(&right.components().count())
                .then_with(|| left.cmp(right))
        });
        ordered.dedup();
        let expanded = ordered.iter().cloned().collect::<HashSet<_>>();
        if expanded != self.watched_directories {
            self.watched_directories = expanded.clone();
            self.service.watch_directories(expanded.clone());
        }
        for path in ordered {
            self.request_directory(path, refresh);
        }
    }

    fn ensure_expanded_directories(&mut self) {
        self.sync_expanded_directories(false);
    }

    fn refresh_expanded_directories(&mut self) {
        self.sync_expanded_directories(true);
    }

    fn request_git_status(&mut self) {
        if self.git_status_request_in_flight {
            return;
        }
        if let Some(root) = self.display_root.as_ref() {
            self.git_status_request_in_flight = true;
            self.service.git_status(root.path.clone());
        }
    }

    fn request_codex_status(&mut self) {
        if self.codex_status_request_in_flight {
            return;
        }
        self.codex_status_request_in_flight = true;
        self.service.codex_status(self.codex_home.clone());
    }

    fn refresh_changed_paths(&mut self, paths: Vec<PathBuf>) {
        let mut directories = HashSet::new();
        for path in paths {
            if self.is_in_display_scope(&path) && self.state.expanded.contains(&path) {
                directories.insert(path.clone());
            }
            if let Some(parent) = path.parent() {
                if self.is_in_display_scope(parent) && self.state.expanded.contains(parent) {
                    directories.insert(parent.to_path_buf());
                }
            }
        }
        for directory in directories {
            self.request_directory(directory, true);
        }
    }

    fn is_in_display_scope(&self, path: &Path) -> bool {
        self.display_root
            .as_ref()
            .map(|root| path == root.path || path.starts_with(&root.path))
            .unwrap_or(true)
    }

    fn exact_root_index(&self, path: &Path) -> Option<usize> {
        self.state.roots.iter().position(|root| root.path == path)
    }

    fn set_display_root(&mut self, path: PathBuf, ensure_expanded: bool) -> bool {
        let root = self
            .state
            .roots
            .iter()
            .find(|root| root.path == path)
            .cloned()
            .unwrap_or_else(|| WorkspaceRoot::new(path.clone()));
        let changed = self.display_root.as_ref() != Some(&root);
        let expanded = (changed || ensure_expanded) && self.state.expanded.insert(path.clone());
        self.display_root = Some(root);
        if changed {
            self.scroll = 0;
            self.selected_path = Some(path.clone());
            self.git_statuses.clear();
        }
        self.selected_root = self.exact_root_index(&path);
        self.ensure_expanded_directories();
        self.request_git_status();
        changed || expanded
    }

    fn apply_context(&mut self, context: PaneContext, reveal_active: bool) -> bool {
        self.context_request_in_flight = false;
        let prior = self.active_context.clone();
        let context_changed = prior.as_ref() != Some(&context);
        let mut changed = context_changed;
        self.active_context = Some(context.clone());

        if let Some(cwd) = context.cwd.clone() {
            let prior_state = self.state.clone();
            // Cosmos intentionally differs from a multi-root editor workspace:
            // the explorer is a permanent view of the active pane's exact
            // working directory. Serialized follow modes are retained only for
            // backward-compatible state loading.
            self.state.follow_mode = FollowMode::Follow;
            let root_changed = self.set_display_root(cwd.clone(), reveal_active);
            changed |= root_changed;
            if root_changed || !self.focused {
                self.selected_path = Some(cwd);
            }
            if self.state != prior_state {
                self.persist();
                changed = true;
            }
        } else if let Some(error) = &context.error {
            self.last_error = Some(error.clone());
            changed = true;
        }
        changed
    }

    fn active_highlight_path(&self) -> Option<&Path> {
        let context = self.active_context.as_ref()?;
        let cwd = context.cwd.as_deref()?;
        self.is_in_display_scope(cwd).then_some(cwd)
    }

    fn rebuild_rows(&mut self, capacity: usize) {
        self.rows = if let Some(root) = &self.display_root {
            let root_index = self
                .exact_root_index(&root.path)
                .unwrap_or(VIRTUAL_ROOT_INDEX);
            self.cache.rows_for_root(&self.state, root, root_index)
        } else {
            // The explorer has no display scope until the active pane context
            // resolves. Historical serialized roots must never flash in the
            // permanent current-directory view.
            vec![]
        };
        if let Some(error) = &self.last_error {
            self.rows.push(ExplorerRow {
                path: None,
                root_index: VIRTUAL_ROOT_INDEX,
                depth: 1,
                label: error.clone(),
                kind: ExplorerRowKind::Error,
                expanded: false,
            });
        }
        self.rendered_capacity = capacity;

        let target_path = if self.focused {
            self.selected_path.as_deref()
        } else {
            self.active_highlight_path()
        };
        if let Some(target_path) = target_path {
            if let Some(index) = self
                .rows
                .iter()
                .position(|row| row.path.as_deref() == Some(target_path))
            {
                if index < self.scroll {
                    self.scroll = index;
                } else if capacity > 0 && index >= self.scroll + capacity {
                    self.scroll = index.saturating_sub(capacity.saturating_sub(1));
                }
            }
        }
        self.scroll = self
            .scroll
            .min(self.rows.len().saturating_sub(capacity.max(1)));
        self.rendered_start = self.scroll;
    }

    fn selected_index(&self) -> Option<usize> {
        let path = self.selected_path.as_deref()?;
        self.rows
            .iter()
            .position(|row| row.path.as_deref() == Some(path))
    }

    fn set_selected_index(&mut self, index: usize) {
        if let Some(row) = self.rows.get(index) {
            self.selected_path = row.path.clone();
            self.selected_root = (row.root_index != VIRTUAL_ROOT_INDEX).then_some(row.root_index);
            self.focused = true;
        }
    }

    fn move_selection(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let current = self.selected_index().unwrap_or(0);
        let next = (current as isize + delta).clamp(0, self.rows.len() as isize - 1) as usize;
        self.set_selected_index(next);
    }

    fn toggle_row(&mut self, index: usize) {
        let row = match self.rows.get(index).cloned() {
            Some(row) => row,
            None => return,
        };
        if !matches!(row.kind, ExplorerRowKind::Root | ExplorerRowKind::Directory) {
            return;
        }
        let path = match row.path {
            Some(path) => path,
            None => return,
        };
        if self.state.expanded.remove(&path) {
            self.scroll = self.scroll.min(index);
        } else {
            self.state.expanded.insert(path.clone());
            self.request_directory(path, true);
        }
        self.persist();
    }

    fn collapse_or_parent(&mut self) {
        let index = match self.selected_index() {
            Some(index) => index,
            None => return,
        };
        let row = match self.rows.get(index).cloned() {
            Some(row) => row,
            None => return,
        };
        if row.expanded {
            self.toggle_row(index);
            return;
        }
        if let Some(path) = row.path.as_deref().and_then(Path::parent) {
            if let Some(parent_index) = self
                .rows
                .iter()
                .position(|candidate| candidate.path.as_deref() == Some(path))
            {
                self.set_selected_index(parent_index);
            }
        }
    }

    fn expand_or_child(&mut self) {
        let index = match self.selected_index() {
            Some(index) => index,
            None => return,
        };
        let row = match self.rows.get(index).cloned() {
            Some(row) => row,
            None => return,
        };
        if matches!(row.kind, ExplorerRowKind::Root | ExplorerRowKind::Directory) && !row.expanded {
            self.toggle_row(index);
        } else if index + 1 < self.rows.len() && self.rows[index + 1].depth > row.depth {
            self.set_selected_index(index + 1);
        }
    }

    fn selected_directory(&self) -> Option<PathBuf> {
        let index = self.selected_index()?;
        let row = self.rows.get(index)?;
        if matches!(row.kind, ExplorerRowKind::Root | ExplorerRowKind::Directory) {
            row.path.clone()
        } else {
            row.path
                .as_deref()
                .and_then(Path::parent)
                .map(Path::to_path_buf)
        }
    }
}

fn truncate_to_width(text: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(text) <= max_width {
        return text.to_string();
    }
    if max_width <= 1 {
        return "…".to_string();
    }
    let mut result = String::new();
    let target = max_width - 1;
    for ch in text.chars() {
        if UnicodeWidthStr::width(result.as_str()) + UnicodeWidthStr::width(ch.to_string().as_str())
            > target
        {
            break;
        }
        result.push(ch);
    }
    result.push('…');
    result
}

impl TermWindow {
    pub fn explorer_width(&self) -> usize {
        self.explorer.total_width(self.dimensions.dpi)
    }

    pub fn terminal_origin_x(&self) -> usize {
        self.explorer_width()
    }

    pub fn status_bar_height(&self) -> usize {
        logical_to_physical(STATUS_BAR_HEIGHT, self.dimensions.dpi)
    }

    pub fn schedule_explorer_tick(&mut self) {
        if self.explorer.tick_scheduled {
            return;
        }
        let window = match self.window.as_ref() {
            Some(window) => window.clone(),
            None => return,
        };
        self.explorer.tick_scheduled = true;
        promise::spawn::spawn(async move {
            smol::Timer::after(SERVICE_POLL_INTERVAL).await;
            window.notify(TermWindowNotif::ExplorerTick);
        })
        .detach();
    }

    pub fn explorer_tick(&mut self) -> bool {
        self.explorer.tick_scheduled = false;
        let mut changed = false;
        while let Some(response) = self.explorer.service.try_recv() {
            match response {
                ServiceResponse::DirectoryListed(listing) => {
                    self.explorer.pending_directories.remove(&listing.path);
                    changed |= self.explorer.cache.apply(listing);
                    // A newly loaded parent can expose persisted expanded
                    // descendants. Queue only those not already cached; do
                    // not rescan the tree from paint or context polling.
                    self.explorer.ensure_expanded_directories();
                }
                ServiceResponse::ContextResolved(context) => {
                    let is_current = self
                        .get_active_pane_no_overlay()
                        .map(|pane| pane.pane_id() == context.pane_id)
                        .unwrap_or(false);
                    if is_current {
                        changed |= self.explorer.apply_context(context, false);
                    } else {
                        self.explorer.context_request_in_flight = false;
                    }
                }
                ServiceResponse::GitStatusLoaded(snapshot) => {
                    self.explorer.git_status_request_in_flight = false;
                    let is_current = self
                        .explorer
                        .display_root
                        .as_ref()
                        .map(|root| root.path == snapshot.requested_root)
                        .unwrap_or(false);
                    if is_current && self.explorer.git_statuses != snapshot.statuses {
                        self.explorer.git_statuses = snapshot.statuses;
                        changed = true;
                    }
                }
                ServiceResponse::CodexStatusLoaded(snapshot) => {
                    self.explorer.codex_status_request_in_flight = false;
                    if self.explorer.codex_status != snapshot {
                        self.explorer.codex_status = snapshot;
                        changed = true;
                    }
                }
                ServiceResponse::DirectoryChanged(paths) => {
                    self.explorer.refresh_changed_paths(paths);
                }
                ServiceResponse::WatcherError(error) => {
                    self.explorer.last_error = Some(error);
                    changed = true;
                }
            }
        }

        let now = Instant::now();
        if !self.explorer.context_request_in_flight
            && now.duration_since(self.explorer.last_context_request) >= CONTEXT_POLL_INTERVAL
        {
            self.request_explorer_context();
        }
        if now.duration_since(self.explorer.last_directory_refresh) >= DIRECTORY_REFRESH_INTERVAL {
            self.explorer.last_directory_refresh = now;
            self.explorer.refresh_expanded_directories();
        }
        if now.duration_since(self.explorer.last_git_status_refresh) >= DIRECTORY_REFRESH_INTERVAL {
            self.explorer.last_git_status_refresh = now;
            self.explorer.request_git_status();
        }
        if now.duration_since(self.explorer.last_codex_status_refresh)
            >= CODEX_STATUS_REFRESH_INTERVAL
        {
            self.explorer.last_codex_status_refresh = now;
            self.explorer.request_codex_status();
        }
        self.schedule_explorer_tick();
        changed
    }

    fn request_explorer_context(&mut self) {
        let pane = match self.get_active_pane_no_overlay() {
            Some(pane) => pane,
            None => return,
        };
        let reported_cwd = pane
            .get_current_working_dir(CachePolicy::AllowStale)
            .and_then(|url| url.to_file_path().ok());
        let last_known_cwd = self
            .explorer
            .active_context
            .as_ref()
            .and_then(|context| context.cwd.clone());
        let request = PaneContextRequest {
            pane_id: pane.pane_id(),
            pane_title: pane.get_title(),
            reported_cwd,
            foreground_process: pane.get_foreground_process_name(CachePolicy::AllowStale),
            tty_name: pane.tty_name(),
            roots: self
                .explorer
                .state
                .roots
                .iter()
                .map(|root| root.path.clone())
                .collect(),
            last_known_cwd,
        };
        self.explorer.context_request_in_flight = true;
        self.explorer.last_context_request = Instant::now();
        self.explorer.service.resolve_context(request);
    }

    pub fn reveal_active_in_explorer(&mut self) {
        if let Some(context) = self.explorer.active_context.clone() {
            self.explorer.apply_context(context, true);
        } else {
            self.request_explorer_context();
        }
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    pub fn toggle_explorer(&mut self) {
        // Retained for command/config compatibility. Cosmos now treats the
        // explorer as a permanent workbench region, so the old toggle focuses
        // it instead of hiding it.
        self.focus_explorer();
    }

    pub fn focus_explorer(&mut self) {
        self.explorer.state.visible = true;
        self.explorer.focused = true;
        if self.explorer.selected_path.is_none() {
            self.explorer.selected_path = self
                .explorer
                .active_context
                .as_ref()
                .and_then(|context| context.cwd.clone());
        }
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    pub fn blur_explorer(&mut self) {
        if self.explorer.focused {
            self.explorer.focused = false;
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
    }

    pub fn cycle_explorer_follow_mode(&mut self) {
        self.explorer.state.follow_mode = FollowMode::Follow;
        self.explorer.persist();
        self.reveal_active_in_explorer();
    }

    pub fn toggle_explorer_lock(&mut self) {
        self.explorer.state.follow_mode = FollowMode::Follow;
        self.explorer.persist();
        self.reveal_active_in_explorer();
    }

    pub fn toggle_explorer_hidden_files(&mut self) {
        self.explorer.state.show_hidden = !self.explorer.state.show_hidden;
        self.explorer.cache.clear();
        self.explorer.pending_directories.clear();
        self.explorer.ensure_expanded_directories();
        self.explorer.persist();
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    pub fn remove_selected_workspace_root(&mut self) {
        let index = match self.explorer.selected_root {
            Some(index) => index,
            None => return,
        };
        if let Some(removed) = self.explorer.state.remove_root(index) {
            self.explorer.selected_path = None;
            self.explorer.selected_root = None;
            if self
                .explorer
                .display_root
                .as_ref()
                .map(|root| root.path == removed.path)
                .unwrap_or(false)
            {
                self.explorer.display_root = self.explorer.state.roots.first().cloned();
            }
            self.explorer.cache.clear();
            self.explorer.ensure_expanded_directories();
            self.explorer.persist();
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
    }

    pub fn move_selected_workspace_root(&mut self, delta: isize) {
        let index = match self.explorer.selected_root {
            Some(index) => index,
            None => return,
        };
        if let Some(destination) = self.explorer.state.move_root(index, delta) {
            self.explorer.selected_root = Some(destination);
            self.explorer.persist();
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
        }
    }

    pub fn apply_explorer_prompt(&mut self, kind: ExplorerPromptKind, value: Option<String>) {
        let value = match value {
            Some(value) if !value.trim().is_empty() => value,
            _ => return,
        };
        match kind {
            ExplorerPromptKind::AddRoot => {
                let path = expand_home(&value, &config::HOME_DIR);
                if !path.is_dir() {
                    self.explorer.last_error = Some(format!(
                        "Workspace root is not a directory: {}",
                        path.display()
                    ));
                    if let Some(window) = self.window.as_ref() {
                        window.invalidate();
                    }
                    return;
                }
                let index = self.explorer.state.add_root(path.clone());
                self.explorer.state.follow_mode = FollowMode::Follow;
                self.explorer.selected_path = Some(path);
                self.explorer.selected_root = Some(index);
                self.explorer.persist();
            }
            ExplorerPromptKind::RenameRoot(index) => {
                self.explorer.state.rename_root(index, value);
                if let (Some(displayed), Some(root)) = (
                    self.explorer.display_root.as_mut(),
                    self.explorer.state.roots.get(index),
                ) {
                    if displayed.path == root.path {
                        displayed.name = root.name.clone();
                    }
                }
                self.explorer.persist();
            }
        }
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    pub fn show_explorer_prompt(&mut self, kind: ExplorerPromptKind) {
        let mux = Mux::get();
        let tab = match mux.get_active_tab_for_window(self.mux_window_id) {
            Some(tab) => tab,
            None => return,
        };
        let window = match self.window.as_ref() {
            Some(window) => window.clone(),
            None => return,
        };
        let description = match kind {
            ExplorerPromptKind::AddRoot => "Add workspace root (absolute path or ~/…)",
            ExplorerPromptKind::RenameRoot(index) => self
                .explorer
                .state
                .roots
                .get(index)
                .map(|root| root.name.as_str())
                .unwrap_or("Rename workspace root"),
        }
        .to_string();
        let (overlay, future) = crate::overlay::start_overlay(self, &tab, move |_tab_id, term| {
            explorer_prompt_overlay(term, window, kind, description)
        });
        self.assign_overlay(tab.tab_id(), overlay);
        promise::spawn::spawn(future).detach();
    }

    pub fn spawn_selected_explorer_directory(&self, split: bool) {
        let cwd = match self.explorer.selected_directory() {
            Some(cwd) => cwd,
            None => return,
        };
        let command = SpawnCommand {
            cwd: Some(cwd),
            domain: SpawnTabDomain::CurrentPaneDomain,
            ..Default::default()
        };
        if split {
            self.spawn_command(
                &command,
                SpawnWhere::SplitPane(SplitRequest {
                    direction: SplitDirection::Horizontal,
                    target_is_second: true,
                    size: SplitSize::Percent(50),
                    top_level: false,
                }),
            );
        } else {
            self.spawn_command(&command, SpawnWhere::NewTab);
        }
    }

    pub fn explorer_key_down(&mut self, key: &KeyCode, modifiers: Modifiers) -> bool {
        if !self.explorer.focused {
            return false;
        }
        let plain_modifiers = modifiers.remove_positional_mods();
        match (key, plain_modifiers) {
            (KeyCode::UpArrow, Modifiers::NONE) => self.explorer.move_selection(-1),
            (KeyCode::DownArrow, Modifiers::NONE) => self.explorer.move_selection(1),
            (KeyCode::LeftArrow, Modifiers::NONE) => self.explorer.collapse_or_parent(),
            (KeyCode::RightArrow, Modifiers::NONE) => self.explorer.expand_or_child(),
            (KeyCode::Char('\r'), Modifiers::NONE) => {
                if let Some(index) = self.explorer.selected_index() {
                    self.explorer.toggle_row(index);
                }
            }
            (KeyCode::Char('\r'), Modifiers::SUPER) => {
                self.spawn_selected_explorer_directory(false)
            }
            (KeyCode::Char('\r'), Modifiers::SHIFT) => self.spawn_selected_explorer_directory(true),
            (KeyCode::Char('\u{1b}'), Modifiers::NONE) => self.explorer.focused = false,
            (KeyCode::Char('\u{7f}'), Modifiers::NONE) => self.remove_selected_workspace_root(),
            (KeyCode::Function(2), Modifiers::NONE) => {
                if let Some(index) = self.explorer.selected_root {
                    self.show_explorer_prompt(ExplorerPromptKind::RenameRoot(index));
                }
            }
            (KeyCode::Char('a'), Modifiers::NONE) => {
                self.show_explorer_prompt(ExplorerPromptKind::AddRoot)
            }
            (KeyCode::Char('r'), Modifiers::NONE) => self.reveal_active_in_explorer(),
            (KeyCode::Char('.'), Modifiers::NONE) => self.toggle_explorer_hidden_files(),
            _ => return false,
        }
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
        true
    }

    fn render_explorer_line(
        &mut self,
        layers: &mut TripleLayerQuadAllocator,
        text: &str,
        top: f32,
        left: f32,
        width: f32,
        height: f32,
        foreground: RgbColor,
        _background: RgbColor,
        bold: bool,
        logical_font_size: f64,
    ) -> anyhow::Result<()> {
        self.render_explorer_text(
            layers,
            text,
            top,
            left,
            width,
            height,
            foreground,
            EXPLORER_UI_FONT_FAMILY,
            bold,
            logical_font_size,
        )
    }

    fn render_explorer_glyph(
        &mut self,
        layers: &mut TripleLayerQuadAllocator,
        glyph: &str,
        top: f32,
        left: f32,
        width: f32,
        height: f32,
        foreground: RgbColor,
    ) -> anyhow::Result<()> {
        self.render_explorer_text(
            layers,
            glyph,
            top,
            left,
            width,
            height,
            foreground,
            "seti",
            false,
            EXPLORER_ICON_FONT_LOGICAL_SIZE,
        )
    }

    fn render_explorer_text(
        &mut self,
        layers: &mut TripleLayerQuadAllocator,
        text: &str,
        top: f32,
        left: f32,
        width: f32,
        height: f32,
        foreground: RgbColor,
        family: &str,
        bold: bool,
        logical_font_size: f64,
    ) -> anyhow::Result<()> {
        let mut attributes = FontAttributes::new(family);
        let configured_font_size = self.config.font_size * self.fonts.get_font_scale();
        let explorer_scale = logical_font_size / configured_font_size;
        attributes.scale = Some(NotNan::new(explorer_scale).unwrap());
        if bold {
            attributes.weight = FontWeight::BOLD;
        }
        let font = self.fonts.resolve_font(&TextStyle {
            font: vec![attributes],
            foreground: None,
        })?;
        let render_metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let top = (top + ((height - render_metrics.cell_size.height as f32) / 2.).max(0.)).round();
        let left = left.round();
        let gl_state = self.render_state.as_ref().unwrap();
        let element = Element::new(&font, ElementContent::Text(text.to_string()));
        let computed = self.compute_element(
            &LayoutContext {
                width: DimensionContext {
                    dpi: self.dimensions.dpi as f32,
                    pixel_max: self.dimensions.pixel_width as f32,
                    pixel_cell: render_metrics.cell_size.width as f32,
                },
                height: DimensionContext {
                    dpi: self.dimensions.dpi as f32,
                    pixel_max: self.dimensions.pixel_height as f32,
                    pixel_cell: render_metrics.cell_size.height as f32,
                },
                bounds: euclid::rect(left, top, width, render_metrics.cell_size.height as f32),
                metrics: &render_metrics,
                gl_state,
                zindex: 0,
            },
            &element,
        )?;

        let cells = match &computed.content {
            crate::termwindow::box_model::ComputedElementContent::Text(cells) => cells,
            _ => unreachable!("text element computed to non-text content"),
        };
        let left_offset = self.dimensions.pixel_width as f32 / -2.;
        let top_offset = self.dimensions.pixel_height as f32 / -2.;
        let mut pos_x = computed.content_rect.min_x().round();
        let color = foreground.to_linear_tuple_rgba();
        for cell in cells {
            if pos_x >= computed.content_rect.max_x() {
                break;
            }
            match cell {
                ElementCell::Sprite(sprite) => {
                    let glyph_width = sprite.coords.width() as f32;
                    let glyph_height = sprite.coords.height() as f32;
                    if pos_x + glyph_width > computed.content_rect.max_x() {
                        break;
                    }
                    let mut quad = layers.allocate(1)?;
                    let sprite_y = computed.content_rect.min_y().round() + top_offset;
                    quad.set_position(
                        pos_x + left_offset,
                        sprite_y,
                        pos_x + left_offset + glyph_width,
                        sprite_y + glyph_height,
                    );
                    quad.set_fg_color(color);
                    quad.set_alt_color_and_mix_value(color, 0.);
                    quad.set_texture(sprite.texture_coords());
                    quad.set_hsv(None);
                    pos_x += glyph_width;
                }
                ElementCell::Glyph(glyph) => {
                    if let Some(texture) = glyph.texture.as_ref() {
                        let pos_y = (computed.content_rect.min_y()
                            - (glyph.y_offset + glyph.bearing_y).get() as f32
                            + computed.baseline)
                            .round()
                            + top_offset;
                        if pos_x + glyph.x_advance.get() as f32 > computed.content_rect.max_x() {
                            break;
                        }
                        let glyph_x =
                            (pos_x + (glyph.x_offset + glyph.bearing_x).get() as f32).round();
                        let glyph_width = texture.coords.size.width as f32 * glyph.scale as f32;
                        let glyph_height = texture.coords.size.height as f32 * glyph.scale as f32;
                        let mut quad = layers.allocate(1)?;
                        quad.set_position(
                            glyph_x + left_offset,
                            pos_y,
                            glyph_x + left_offset + glyph_width,
                            pos_y + glyph_height,
                        );
                        quad.set_fg_color(color);
                        quad.set_alt_color_and_mix_value(color, 0.);
                        quad.set_texture(texture.texture_coords());
                        quad.set_has_color(glyph.has_color);
                        quad.set_hsv(None);
                    }
                    pos_x += glyph.x_advance.get() as f32;
                }
            }
        }
        Ok(())
    }

    fn draw_explorer_icon(
        &self,
        layers: &mut TripleLayerQuadAllocator,
        icon: &'static [Poly],
        x: usize,
        y: usize,
        size: usize,
        color: RgbColor,
    ) -> anyhow::Result<()> {
        self.poly_quad(
            layers,
            1,
            euclid::point2(x as f32, y as f32),
            icon,
            1,
            euclid::size2(size as f32, size as f32),
            color.to_linear_tuple_rgba(),
        )?;
        Ok(())
    }

    fn explorer_item_hovered(&self, expected: ExplorerUiItem) -> bool {
        matches!(
            self.last_ui_item.as_ref().map(|item| &item.item_type),
            Some(UIItemType::Explorer(current)) if *current == expected
        )
    }

    pub fn paint_explorer(&mut self, layers: &mut TripleLayerQuadAllocator) -> anyhow::Result<()> {
        let dpi = self.dimensions.dpi;
        let scale = |value| logical_to_physical(value, dpi);
        let divider_width = scale(DIVIDER_WIDTH);
        let divider_hit_width = scale(DIVIDER_HIT_WIDTH);
        let title_height = scale(TITLE_HEIGHT);
        let row_height = scale(ROW_HEIGHT);
        let title_left = scale(TITLE_LEFT);
        let tree_left = scale(TREE_LEFT);
        let tree_indent = scale(TREE_INDENT);
        let icon_size = scale(ICON_SIZE);
        let action_size = scale(ACTION_SIZE);
        let border = self.get_os_border();
        let width = scale(self.explorer.state.width_px);
        let top = border.top.get();
        let bottom = self
            .dimensions
            .pixel_height
            .saturating_sub(border.bottom.get() + self.status_bar_height());
        let tree_top = top + title_height;
        let tree_bottom = bottom;
        let tree_height = tree_bottom.saturating_sub(tree_top);
        let capacity = tree_height / row_height;
        self.explorer.rebuild_rows(capacity);

        self.filled_rectangle(
            layers,
            0,
            euclid::rect(0., top as f32, width as f32, (bottom - top) as f32),
            SIDEBAR_BG.to_linear_tuple_rgba(),
        )?;
        self.filled_rectangle(
            layers,
            2,
            euclid::rect(
                width as f32,
                top as f32,
                divider_width as f32,
                (bottom - top) as f32,
            ),
            DIVIDER.to_linear_tuple_rgba(),
        )?;
        self.ui_items.push(UIItem {
            x: 0,
            y: top,
            width,
            height: bottom - top,
            item_type: UIItemType::Explorer(ExplorerUiItem::Surface),
        });

        let action_x = width.saturating_sub(action_size + scale(8));
        let action_y = top + (title_height - action_size) / 2;
        let action = ExplorerHeaderAction::RevealActive;
        if self.explorer_item_hovered(ExplorerUiItem::Header(action)) {
            self.filled_rectangle(
                layers,
                0,
                euclid::rect(
                    action_x as f32,
                    action_y as f32,
                    action_size as f32,
                    action_size as f32,
                ),
                HOVER_BG.to_linear_tuple_rgba(),
            )?;
        }
        let dot_color = MUTED;
        for offset in [6usize, 10, 14].iter().copied().map(scale) {
            self.filled_rectangle(
                layers,
                1,
                euclid::rect(
                    (action_x + offset) as f32,
                    (action_y + action_size / 2) as f32,
                    scale(2) as f32,
                    scale(2) as f32,
                ),
                dot_color.to_linear_tuple_rgba(),
            )?;
        }
        self.ui_items.push(UIItem {
            x: action_x,
            y: action_y,
            width: action_size,
            height: action_size,
            item_type: UIItemType::Explorer(ExplorerUiItem::Header(action)),
        });

        let header_text_width = action_x.saturating_sub(title_left + scale(8));
        self.render_explorer_line(
            layers,
            "EXPLORER",
            top as f32,
            title_left as f32,
            header_text_width as f32,
            title_height as f32,
            TEXT,
            SIDEBAR_BG,
            false,
            EXPLORER_HEADER_FONT_LOGICAL_SIZE,
        )?;

        let active_path = self.explorer.active_highlight_path().map(Path::to_path_buf);
        let selected_path = self.explorer.selected_path.clone();
        let start = self.explorer.rendered_start;
        let end = (start + capacity).min(self.explorer.rows.len());
        for (screen_row, row_index) in (start..end).enumerate() {
            let row = self.explorer.rows[row_index].clone();
            let y = tree_top + screen_row * row_height;
            let hovered = self.explorer_item_hovered(ExplorerUiItem::Row(row_index));
            let is_active = row.path == active_path;
            let is_selected = self.explorer.focused && row.path == selected_path;
            let git_status = row
                .path
                .as_ref()
                .and_then(|path| self.explorer.git_statuses.get(path))
                .copied();
            let background = if row.kind == ExplorerRowKind::Root {
                if hovered {
                    HOVER_BG
                } else {
                    SIDEBAR_BG
                }
            } else if is_selected {
                ACTIVE_SELECTION_BG
            } else if row.path == selected_path || is_active {
                INACTIVE_SELECTION_BG
            } else if hovered {
                HOVER_BG
            } else {
                SIDEBAR_BG
            };
            if background != SIDEBAR_BG {
                self.filled_rectangle(
                    layers,
                    0,
                    euclid::rect(0., y as f32, width as f32, row_height as f32),
                    background.to_linear_tuple_rgba(),
                )?;
            }

            if row.kind == ExplorerRowKind::Root {
                self.filled_rectangle(
                    layers,
                    0,
                    euclid::rect(
                        0.,
                        (y + row_height - divider_width) as f32,
                        width as f32,
                        divider_width as f32,
                    ),
                    DIVIDER.to_linear_tuple_rgba(),
                )?;
                self.draw_explorer_icon(
                    layers,
                    if row.expanded {
                        CHEVRON_DOWN
                    } else {
                        CHEVRON_RIGHT
                    },
                    tree_left,
                    y + (row_height - icon_size) / 2,
                    icon_size,
                    TEXT,
                )?;
                let root_text_left = scale(24);
                let max_cells = width.saturating_sub(root_text_left + scale(8)) / scale(8);
                self.render_explorer_line(
                    layers,
                    &truncate_to_width(&row.label.to_uppercase(), max_cells),
                    y as f32,
                    root_text_left as f32,
                    width.saturating_sub(root_text_left + scale(8)) as f32,
                    row_height as f32,
                    TEXT,
                    background,
                    true,
                    EXPLORER_BODY_FONT_LOGICAL_SIZE,
                )?;
            } else {
                let depth = row.depth.saturating_sub(1);
                for guide in 0..depth {
                    let guide_x = tree_left + icon_size / 2 + guide * tree_indent;
                    self.filled_rectangle(
                        layers,
                        0,
                        euclid::rect(
                            guide_x as f32,
                            y as f32,
                            divider_width as f32,
                            row_height as f32,
                        ),
                        INDENT_GUIDE.to_linear_tuple_rgba(),
                    )?;
                }

                let twistie_x = tree_left + depth * tree_indent;
                let icon_y = y + (row_height - icon_size) / 2;
                if matches!(row.kind, ExplorerRowKind::Directory) {
                    self.draw_explorer_icon(
                        layers,
                        if row.expanded {
                            CHEVRON_DOWN
                        } else {
                            CHEVRON_RIGHT
                        },
                        twistie_x,
                        icon_y,
                        icon_size,
                        TEXT,
                    )?;
                } else if row.kind == ExplorerRowKind::File {
                    let (glyph, color) = row
                        .path
                        .as_deref()
                        .map(explorer_file_icon)
                        .unwrap_or(("\u{e64e}", RgbColor::new_8bpc(212, 215, 214)));
                    self.render_explorer_glyph(
                        layers,
                        glyph,
                        y as f32,
                        twistie_x as f32,
                        icon_size as f32,
                        row_height as f32,
                        color,
                    )?;
                }

                let text_left = twistie_x + icon_size + scale(2);
                let foreground = match row.kind {
                    ExplorerRowKind::Error => ERROR,
                    ExplorerRowKind::Loading | ExplorerRowKind::Truncated => MUTED,
                    _ if is_selected => RgbColor::new_8bpc(255, 255, 255),
                    _ => TEXT,
                };
                let text_right = if git_status.is_some() {
                    width.saturating_sub(scale(28))
                } else {
                    width.saturating_sub(scale(4))
                };
                let max_cells = text_right.saturating_sub(text_left + scale(4)) / scale(8);
                self.render_explorer_line(
                    layers,
                    &truncate_to_width(&row.label, max_cells),
                    y as f32,
                    text_left as f32,
                    text_right.saturating_sub(text_left) as f32,
                    row_height as f32,
                    foreground,
                    background,
                    false,
                    EXPLORER_BODY_FONT_LOGICAL_SIZE,
                )?;
                if let Some(status) = git_status {
                    self.render_explorer_line(
                        layers,
                        status.label(),
                        y as f32,
                        width.saturating_sub(scale(24)) as f32,
                        scale(20) as f32,
                        row_height as f32,
                        git_status_color(status),
                        background,
                        false,
                        EXPLORER_BODY_FONT_LOGICAL_SIZE,
                    )?;
                }
            }
            self.ui_items.push(UIItem {
                x: 0,
                y,
                width,
                height: row_height,
                item_type: UIItemType::Explorer(ExplorerUiItem::Row(row_index)),
            });
        }

        if self.explorer.rows.len() > capacity && capacity > 0 {
            let thumb_height = (tree_height * capacity / self.explorer.rows.len()).max(scale(20));
            let max_scroll = self.explorer.rows.len().saturating_sub(capacity);
            let thumb_travel = tree_height.saturating_sub(thumb_height);
            let thumb_top = tree_top
                + if max_scroll == 0 {
                    0
                } else {
                    thumb_travel * self.explorer.rendered_start / max_scroll
                };
            self.filled_rectangle(
                layers,
                2,
                euclid::rect(
                    width.saturating_sub(scale(SCROLLBAR_SIZE)) as f32,
                    thumb_top as f32,
                    scale(SCROLLBAR_SIZE) as f32,
                    thumb_height as f32,
                ),
                SCROLLBAR.to_linear_tuple_rgba(),
            )?;
        }

        self.ui_items.push(UIItem {
            x: width.saturating_sub(divider_hit_width / 2),
            y: top,
            width: divider_hit_width,
            height: bottom - top,
            item_type: UIItemType::Explorer(ExplorerUiItem::Divider),
        });
        Ok(())
    }

    pub fn paint_status_bar(
        &mut self,
        layers: &mut TripleLayerQuadAllocator,
    ) -> anyhow::Result<()> {
        let dpi = self.dimensions.dpi;
        let scale = |value| logical_to_physical(value, dpi);
        let border = self.get_os_border();
        let height = self.status_bar_height();
        let bottom = self
            .dimensions
            .pixel_height
            .saturating_sub(border.bottom.get());
        let top = bottom.saturating_sub(height);
        let width = self.dimensions.pixel_width;

        self.filled_rectangle(
            layers,
            0,
            euclid::rect(0., top as f32, width as f32, height as f32),
            STATUS_BAR_BG.to_linear_tuple_rgba(),
        )?;
        self.filled_rectangle(
            layers,
            2,
            euclid::rect(0., top as f32, width as f32, scale(1) as f32),
            STATUS_BAR_BORDER.to_linear_tuple_rgba(),
        )?;

        self.render_explorer_line(
            layers,
            "●",
            top as f32,
            scale(8) as f32,
            scale(14) as f32,
            height as f32,
            if self.explorer.codex_status.active_loops > 0 {
                STATUS_BAR_LIVE
            } else {
                MUTED
            },
            STATUS_BAR_BG,
            false,
            STATUS_BAR_FONT_LOGICAL_SIZE,
        )?;

        let usage = truncate_to_width(&self.explorer.codex_status.usage_label(), 42);
        let usage_left = scale(26);
        let usage_logical_width = (UnicodeWidthStr::width(usage.as_str()) * 7 + 12).clamp(120, 300);
        let usage_width = scale(usage_logical_width).min(width.saturating_sub(usage_left));
        self.render_explorer_line(
            layers,
            &usage,
            top as f32,
            usage_left as f32,
            usage_width as f32,
            height as f32,
            STATUS_BAR_TEXT,
            STATUS_BAR_BG,
            false,
            STATUS_BAR_FONT_LOGICAL_SIZE,
        )?;

        let loop_label = match self.explorer.codex_status.active_loops {
            1 => "1 loop".to_string(),
            count => format!("{count} loops"),
        };
        let loops_left = usage_left + usage_width + scale(4);
        let loops_logical_width =
            (UnicodeWidthStr::width(loop_label.as_str()) * 7 + 8).clamp(48, 80);
        let loops_width = scale(loops_logical_width).min(width.saturating_sub(loops_left));
        if loops_left < width {
            self.render_explorer_line(
                layers,
                &loop_label,
                top as f32,
                loops_left as f32,
                loops_width as f32,
                height as f32,
                STATUS_BAR_TEXT,
                STATUS_BAR_BG,
                false,
                STATUS_BAR_FONT_LOGICAL_SIZE,
            )?;
        }

        if let Some(reset) = self.explorer.codex_status.reset_label(SystemTime::now()) {
            let reset_width = scale(120).min(width);
            let reset_left = width.saturating_sub(reset_width + scale(8));
            if reset_left
                > loops_left
                    .saturating_add(loops_width)
                    .saturating_add(scale(8))
            {
                self.render_explorer_line(
                    layers,
                    &reset,
                    top as f32,
                    reset_left as f32,
                    reset_width as f32,
                    height as f32,
                    MUTED,
                    STATUS_BAR_BG,
                    false,
                    STATUS_BAR_FONT_LOGICAL_SIZE,
                )?;
            }
        }

        self.ui_items.push(UIItem {
            x: 0,
            y: top,
            width,
            height,
            item_type: UIItemType::StatusBar,
        });
        Ok(())
    }

    pub fn drag_explorer_divider(&mut self, event: &MouseEvent, context: &dyn WindowOps) {
        let width = physical_to_logical(event.coords.x.max(0) as usize, self.dimensions.dpi)
            .clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
        if width != self.explorer.state.width_px {
            self.explorer.state.width_px = width;
            self.explorer.persist();
            if let Some(window) = self.window.as_ref().cloned() {
                let dimensions = self.dimensions;
                self.apply_dimensions(&dimensions, None, &window);
            }
            context.invalidate();
        }
        context.set_cursor(Some(MouseCursor::SizeLeftRight));
    }

    pub fn mouse_event_explorer(
        &mut self,
        item: ExplorerUiItem,
        event: MouseEvent,
        context: &dyn WindowOps,
    ) {
        match item {
            ExplorerUiItem::Divider => {
                context.set_cursor(Some(MouseCursor::SizeLeftRight));
                if event.kind == MouseEventKind::Press(MousePress::Left) {
                    let divider_hit_width =
                        logical_to_physical(DIVIDER_HIT_WIDTH, self.dimensions.dpi);
                    let ui_item = UIItem {
                        x: logical_to_physical(self.explorer.state.width_px, self.dimensions.dpi),
                        y: 0,
                        width: divider_hit_width,
                        height: self.dimensions.pixel_height,
                        item_type: UIItemType::Explorer(ExplorerUiItem::Divider),
                    };
                    self.dragging.replace((ui_item, event));
                }
            }
            ExplorerUiItem::Header(action) => {
                context.set_cursor(Some(MouseCursor::Hand));
                if event.kind == MouseEventKind::Press(MousePress::Left) {
                    match action {
                        ExplorerHeaderAction::RevealActive => self.reveal_active_in_explorer(),
                    }
                }
            }
            ExplorerUiItem::Row(index) => {
                context.set_cursor(Some(MouseCursor::Arrow));
                match event.kind {
                    MouseEventKind::Press(MousePress::Left) => {
                        self.explorer.set_selected_index(index);
                        self.explorer.focused = true;
                        let double_click = self
                            .last_mouse_click
                            .as_ref()
                            .map(|click| click.streak >= 2)
                            .unwrap_or(false);
                        if double_click {
                            self.spawn_selected_explorer_directory(false);
                        } else {
                            self.explorer.toggle_row(index);
                        }
                        context.invalidate();
                    }
                    MouseEventKind::VertWheel(delta) => {
                        let max_scroll = self
                            .explorer
                            .rows
                            .len()
                            .saturating_sub(self.explorer.rendered_capacity.max(1));
                        if delta > 0 {
                            self.explorer.scroll =
                                self.explorer.scroll.saturating_sub(delta as usize);
                        } else {
                            self.explorer.scroll =
                                (self.explorer.scroll + (-delta) as usize).min(max_scroll);
                        }
                        context.invalidate();
                    }
                    _ => {}
                }
            }
            ExplorerUiItem::Surface => {
                context.set_cursor(Some(MouseCursor::Arrow));
                if event.kind == MouseEventKind::Press(MousePress::Left) {
                    self.explorer.focused = true;
                    context.invalidate();
                }
            }
        }
    }
}

struct ExplorerPromptHost {
    history: BasicHistory,
}

impl ExplorerPromptHost {
    fn new() -> Self {
        Self {
            history: BasicHistory::default(),
        }
    }
}

impl LineEditorHost for ExplorerPromptHost {
    fn history(&mut self) -> &mut dyn History {
        &mut self.history
    }

    fn resolve_action(
        &mut self,
        event: &InputEvent,
        editor: &mut LineEditor<'_>,
    ) -> Option<Action> {
        let (line, _) = editor.get_line_and_cursor();
        if line.is_empty()
            && matches!(
                event,
                InputEvent::Key(TermKeyEvent {
                    key: TermKeyCode::Escape,
                    ..
                })
            )
        {
            Some(Action::Cancel)
        } else {
            None
        }
    }
}

fn explorer_prompt_overlay(
    mut term: mux::termwiztermtab::TermWizTerminal,
    window: Window,
    kind: ExplorerPromptKind,
    description: String,
) -> anyhow::Result<()> {
    term.no_grab_mouse_in_raw_mode();
    term.render(&[
        Change::Text(description),
        Change::Text("\r\n".to_string()),
        Change::AllAttributes(CellAttributes::default()),
    ])?;
    let mut host = ExplorerPromptHost::new();
    let mut editor = LineEditor::new(&mut term);
    editor.set_prompt("> ");
    let value = editor.read_line(&mut host)?;
    promise::spawn::spawn_into_main_thread(async move {
        window.notify(TermWindowNotif::ExplorerPromptResult { kind, value });
        anyhow::Result::<()>::Ok(())
    })
    .detach();
    Ok(())
}
