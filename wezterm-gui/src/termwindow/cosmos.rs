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
    ancestors_from_root, expand_home, DirectoryCache, ExplorerRow, ExplorerRowKind, ExplorerState,
    FollowMode, PaneContext, PaneContextRequest, ServiceResponse, WorkspaceRoot, WorkspaceService,
    MAX_SIDEBAR_WIDTH, MIN_SIDEBAR_WIDTH,
};
use mux::pane::CachePolicy;
use mux::tab::{SplitDirection, SplitRequest, SplitSize};
use mux::Mux;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use termwiz::cell::CellAttributes;
use termwiz::color::RgbColor;
use termwiz::input::{InputEvent, KeyCode as TermKeyCode, KeyEvent as TermKeyEvent};
use termwiz::lineedit::{Action, BasicHistory, History, LineEditor, LineEditorHost};
use termwiz::surface::Change;
use termwiz::terminal::Terminal;
use unicode_width::UnicodeWidthStr;

const DIVIDER_WIDTH: usize = 1;
const DIVIDER_HIT_WIDTH: usize = 7;
const TITLE_HEIGHT: usize = 35;
const ROW_HEIGHT: usize = 22;
const TITLE_LEFT: usize = 20;
const TREE_LEFT: usize = 8;
const TREE_INDENT: usize = 8;
const ICON_SIZE: usize = 16;
const ACTION_SIZE: usize = 22;
const VIRTUAL_ROOT_INDEX: usize = usize::MAX;
const CONTEXT_POLL_INTERVAL: Duration = Duration::from_millis(250);
const DIRECTORY_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

const SIDEBAR_BG: RgbColor = RgbColor::new_8bpc(24, 24, 24);
const DIVIDER: RgbColor = RgbColor::new_8bpc(43, 43, 43);
const TEXT: RgbColor = RgbColor::new_8bpc(204, 204, 204);
const MUTED: RgbColor = RgbColor::new_8bpc(157, 157, 157);
const ICON: RgbColor = RgbColor::new_8bpc(200, 200, 200);
const INDENT_GUIDE: RgbColor = RgbColor::new_8bpc(50, 50, 50);
const HOVER_BG: RgbColor = RgbColor::new_8bpc(42, 45, 46);
const INACTIVE_SELECTION_BG: RgbColor = RgbColor::new_8bpc(55, 55, 61);
const ACTIVE_SELECTION_BG: RgbColor = RgbColor::new_8bpc(4, 57, 94);
const ACCENT: RgbColor = RgbColor::new_8bpc(0, 120, 212);
const ERROR: RgbColor = RgbColor::new_8bpc(248, 128, 112);

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

const FOLDER_ICON: &[Poly] = &[Poly {
    path: &[
        PolyCommand::MoveTo(BlockCoord::Frac(1, 8), BlockCoord::Frac(2, 8)),
        PolyCommand::LineTo(BlockCoord::Frac(3, 8), BlockCoord::Frac(2, 8)),
        PolyCommand::LineTo(BlockCoord::Frac(4, 8), BlockCoord::Frac(3, 8)),
        PolyCommand::LineTo(BlockCoord::Frac(7, 8), BlockCoord::Frac(3, 8)),
        PolyCommand::LineTo(BlockCoord::Frac(7, 8), BlockCoord::Frac(7, 8)),
        PolyCommand::LineTo(BlockCoord::Frac(1, 8), BlockCoord::Frac(7, 8)),
        PolyCommand::Close,
    ],
    intensity: BlockAlpha::Full,
    style: PolyStyle::OutlineThin,
}];

const FILE_ICON: &[Poly] = &[Poly {
    path: &[
        PolyCommand::MoveTo(BlockCoord::Frac(2, 8), BlockCoord::Frac(1, 8)),
        PolyCommand::LineTo(BlockCoord::Frac(5, 8), BlockCoord::Frac(1, 8)),
        PolyCommand::LineTo(BlockCoord::Frac(7, 8), BlockCoord::Frac(3, 8)),
        PolyCommand::LineTo(BlockCoord::Frac(7, 8), BlockCoord::Frac(7, 8)),
        PolyCommand::LineTo(BlockCoord::Frac(2, 8), BlockCoord::Frac(7, 8)),
        PolyCommand::Close,
        PolyCommand::MoveTo(BlockCoord::Frac(5, 8), BlockCoord::Frac(1, 8)),
        PolyCommand::LineTo(BlockCoord::Frac(5, 8), BlockCoord::Frac(3, 8)),
        PolyCommand::LineTo(BlockCoord::Frac(7, 8), BlockCoord::Frac(3, 8)),
    ],
    intensity: BlockAlpha::Full,
    style: PolyStyle::OutlineThin,
}];

const ADD_ROOT_ICON: &[Poly] = &[
    Poly {
        path: &[
            PolyCommand::MoveTo(BlockCoord::Frac(1, 8), BlockCoord::Frac(3, 8)),
            PolyCommand::LineTo(BlockCoord::Frac(3, 8), BlockCoord::Frac(3, 8)),
            PolyCommand::LineTo(BlockCoord::Frac(4, 8), BlockCoord::Frac(4, 8)),
            PolyCommand::LineTo(BlockCoord::Frac(7, 8), BlockCoord::Frac(4, 8)),
            PolyCommand::LineTo(BlockCoord::Frac(7, 8), BlockCoord::Frac(7, 8)),
            PolyCommand::LineTo(BlockCoord::Frac(1, 8), BlockCoord::Frac(7, 8)),
            PolyCommand::Close,
        ],
        intensity: BlockAlpha::Full,
        style: PolyStyle::OutlineThin,
    },
    Poly {
        path: &[
            PolyCommand::MoveTo(BlockCoord::Frac(5, 8), BlockCoord::Frac(1, 8)),
            PolyCommand::LineTo(BlockCoord::Frac(5, 8), BlockCoord::Frac(4, 8)),
            PolyCommand::MoveTo(BlockCoord::Frac(3, 8), BlockCoord::Frac(2, 8)),
            PolyCommand::LineTo(BlockCoord::Frac(7, 8), BlockCoord::Frac(2, 8)),
        ],
        intensity: BlockAlpha::Full,
        style: PolyStyle::OutlineThin,
    },
];

const REVEAL_ICON: &[Poly] = &[
    Poly {
        path: &[
            PolyCommand::MoveTo(BlockCoord::Frac(4, 8), BlockCoord::Frac(1, 8)),
            PolyCommand::LineTo(BlockCoord::Frac(4, 8), BlockCoord::Frac(3, 8)),
            PolyCommand::MoveTo(BlockCoord::Frac(4, 8), BlockCoord::Frac(5, 8)),
            PolyCommand::LineTo(BlockCoord::Frac(4, 8), BlockCoord::Frac(7, 8)),
            PolyCommand::MoveTo(BlockCoord::Frac(1, 8), BlockCoord::Frac(4, 8)),
            PolyCommand::LineTo(BlockCoord::Frac(3, 8), BlockCoord::Frac(4, 8)),
            PolyCommand::MoveTo(BlockCoord::Frac(5, 8), BlockCoord::Frac(4, 8)),
            PolyCommand::LineTo(BlockCoord::Frac(7, 8), BlockCoord::Frac(4, 8)),
        ],
        intensity: BlockAlpha::Full,
        style: PolyStyle::OutlineThin,
    },
    Poly {
        path: &[
            PolyCommand::MoveTo(BlockCoord::Frac(3, 8), BlockCoord::Frac(3, 8)),
            PolyCommand::LineTo(BlockCoord::Frac(5, 8), BlockCoord::Frac(3, 8)),
            PolyCommand::LineTo(BlockCoord::Frac(5, 8), BlockCoord::Frac(5, 8)),
            PolyCommand::LineTo(BlockCoord::Frac(3, 8), BlockCoord::Frac(5, 8)),
            PolyCommand::Close,
        ],
        intensity: BlockAlpha::Full,
        style: PolyStyle::OutlineThin,
    },
];

const FOLLOW_ICON: &[Poly] = &[Poly {
    path: &[
        PolyCommand::MoveTo(BlockCoord::Frac(1, 8), BlockCoord::Frac(3, 8)),
        PolyCommand::LineTo(BlockCoord::Frac(6, 8), BlockCoord::Frac(3, 8)),
        PolyCommand::LineTo(BlockCoord::Frac(5, 8), BlockCoord::Frac(2, 8)),
        PolyCommand::MoveTo(BlockCoord::Frac(7, 8), BlockCoord::Frac(5, 8)),
        PolyCommand::LineTo(BlockCoord::Frac(2, 8), BlockCoord::Frac(5, 8)),
        PolyCommand::LineTo(BlockCoord::Frac(3, 8), BlockCoord::Frac(6, 8)),
    ],
    intensity: BlockAlpha::Full,
    style: PolyStyle::OutlineThin,
}];

const HIDDEN_ICON: &[Poly] = &[
    Poly {
        path: &[
            PolyCommand::MoveTo(BlockCoord::Frac(1, 8), BlockCoord::Frac(4, 8)),
            PolyCommand::QuadTo {
                control: (BlockCoord::Frac(4, 8), BlockCoord::Frac(1, 8)),
                to: (BlockCoord::Frac(7, 8), BlockCoord::Frac(4, 8)),
            },
            PolyCommand::QuadTo {
                control: (BlockCoord::Frac(4, 8), BlockCoord::Frac(7, 8)),
                to: (BlockCoord::Frac(1, 8), BlockCoord::Frac(4, 8)),
            },
        ],
        intensity: BlockAlpha::Full,
        style: PolyStyle::OutlineThin,
    },
    Poly {
        path: &[
            PolyCommand::MoveTo(BlockCoord::Frac(4, 8), BlockCoord::Frac(3, 8)),
            PolyCommand::LineTo(BlockCoord::Frac(4, 8), BlockCoord::Frac(5, 8)),
        ],
        intensity: BlockAlpha::Full,
        style: PolyStyle::Outline,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerHeaderAction {
    AddRoot,
    Reveal,
    CycleFollowMode,
    ToggleHidden,
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
    last_context_request: Instant,
    last_directory_refresh: Instant,
    tick_scheduled: bool,
    last_error: Option<String>,
}

impl ExplorerUi {
    pub fn load() -> Self {
        let state_path = config::DATA_DIR.join("workspace-state.json");
        let (mut state, last_error) = match ExplorerState::load(&state_path) {
            Ok(state) => (state, None),
            Err(err) => (
                ExplorerState::default(),
                Some(format!("Unable to load explorer state: {err}")),
            ),
        };
        let display_root = state.roots.first().cloned();
        if let Some(root) = &display_root {
            state.expanded.insert(root.path.clone());
        }
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
            last_context_request: Instant::now()
                .checked_sub(CONTEXT_POLL_INTERVAL)
                .unwrap_or_else(Instant::now),
            last_directory_refresh: Instant::now(),
            tick_scheduled: false,
            last_error,
        }
    }

    pub fn total_width(&self) -> usize {
        if self.state.visible {
            self.state.width_px + DIVIDER_WIDTH
        } else {
            0
        }
    }

    fn persist(&mut self) {
        if let Err(err) = self.state.save(&self.state_path) {
            self.last_error = Some(format!("Unable to save explorer state: {err}"));
        }
    }

    fn request_directory(&mut self, path: PathBuf) {
        if self.pending_directories.insert(path.clone()) {
            self.cache.mark_loading(path.clone());
            self.service.list_directory(path, self.state.show_hidden);
        }
    }

    fn request_expanded_directories(&mut self) {
        let display_root = self.display_root.as_ref().map(|root| root.path.clone());
        let expanded = self
            .state
            .expanded
            .iter()
            .filter(|path| {
                path.is_dir()
                    && display_root
                        .as_ref()
                        .map(|root| path.as_path() == root || path.starts_with(root))
                        .unwrap_or(true)
            })
            .cloned()
            .collect::<HashSet<_>>();
        if expanded != self.watched_directories {
            self.watched_directories = expanded.clone();
            self.service.watch_directories(expanded.clone());
        }
        for path in expanded {
            self.request_directory(path);
        }
    }

    fn refresh_expanded_directories(&mut self) {
        self.request_expanded_directories();
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
            self.request_directory(directory);
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
        }
        self.selected_root = self.exact_root_index(&path);
        self.request_expanded_directories();
        changed || expanded
    }

    fn apply_context(&mut self, context: PaneContext, reveal_even_if_locked: bool) -> bool {
        self.context_request_in_flight = false;
        let prior = self.active_context.clone();
        let context_changed = prior.as_ref() != Some(&context);
        let mut changed = context_changed;
        self.active_context = Some(context.clone());

        if let Some(cwd) = context.cwd.clone() {
            let prior_state = self.state.clone();
            let should_update_root = reveal_even_if_locked
                || self.state.follow_mode != FollowMode::Locked
                || self.display_root.is_none();
            if should_update_root {
                let root_path = match self.state.follow_mode {
                    FollowMode::Follow | FollowMode::Locked => cwd.clone(),
                    FollowMode::ProjectFollow => context
                        .project_root
                        .clone()
                        .or(context.workspace_root.clone())
                        .unwrap_or_else(|| cwd.clone()),
                };
                let root_changed = self.set_display_root(root_path.clone(), reveal_even_if_locked);
                changed |= root_changed;

                if self.state.follow_mode == FollowMode::ProjectFollow
                    && cwd.starts_with(&root_path)
                    && (root_changed || context_changed || reveal_even_if_locked)
                {
                    for ancestor in ancestors_from_root(&root_path, &cwd) {
                        self.state.expanded.insert(ancestor);
                    }
                }
                if root_changed || !self.focused {
                    self.selected_path =
                        Some(if self.state.follow_mode == FollowMode::ProjectFollow {
                            cwd.clone()
                        } else {
                            root_path
                        });
                }
            }
            self.request_expanded_directories();
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
            self.cache.rows(&self.state)
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
            self.request_directory(path);
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
        self.explorer.total_width()
    }

    pub fn terminal_origin_x(&self) -> usize {
        self.explorer_width()
    }

    pub fn schedule_explorer_tick(&mut self) {
        if !self.explorer.state.visible || self.explorer.tick_scheduled {
            return;
        }
        let window = match self.window.as_ref() {
            Some(window) => window.clone(),
            None => return,
        };
        self.explorer.tick_scheduled = true;
        promise::spawn::spawn(async move {
            smol::Timer::after(CONTEXT_POLL_INTERVAL).await;
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
        self.schedule_explorer_tick();
        changed
    }

    fn request_explorer_context(&mut self) {
        let pane = match self.get_active_pane_no_overlay() {
            Some(pane) => pane,
            None => return,
        };
        let reported_cwd = pane
            .get_current_working_dir(CachePolicy::FetchImmediate)
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
            foreground_process: pane.get_foreground_process_name(CachePolicy::FetchImmediate),
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
        self.explorer.state.visible = !self.explorer.state.visible;
        if !self.explorer.state.visible {
            self.explorer.focused = false;
            self.explorer.watched_directories.clear();
            self.explorer.service.watch_directories(HashSet::new());
        } else {
            self.explorer.request_expanded_directories();
        }
        self.explorer.persist();
        if let Some(window) = self.window.as_ref().cloned() {
            let dimensions = self.dimensions;
            self.apply_dimensions(&dimensions, None, &window);
            window.invalidate();
        }
        self.schedule_explorer_tick();
    }

    pub fn focus_explorer(&mut self) {
        if !self.explorer.state.visible {
            self.toggle_explorer();
        }
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

    pub fn cycle_explorer_follow_mode(&mut self) {
        self.explorer.state.follow_mode = self.explorer.state.follow_mode.next();
        self.explorer.persist();
        if self.explorer.state.follow_mode != FollowMode::Locked {
            self.reveal_active_in_explorer();
        }
    }

    pub fn toggle_explorer_hidden_files(&mut self) {
        self.explorer.state.show_hidden = !self.explorer.state.show_hidden;
        self.explorer.cache.clear();
        self.explorer.pending_directories.clear();
        self.explorer.request_expanded_directories();
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
            self.explorer.request_expanded_directories();
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
                self.explorer.state.follow_mode = FollowMode::Locked;
                self.explorer.set_display_root(path.clone(), true);
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
        if !self.explorer.state.visible || !self.explorer.focused {
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
            (KeyCode::Char('f'), Modifiers::NONE) => {
                self.explorer.state.follow_mode = FollowMode::Follow;
                self.explorer.persist();
                self.reveal_active_in_explorer();
            }
            (KeyCode::Char('p'), Modifiers::NONE) => {
                self.explorer.state.follow_mode = FollowMode::ProjectFollow;
                self.explorer.persist();
                self.reveal_active_in_explorer();
            }
            (KeyCode::Char('l'), Modifiers::NONE) => {
                self.explorer.state.follow_mode = FollowMode::Locked;
                self.explorer.persist();
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
    ) -> anyhow::Result<()> {
        let mut attributes = FontAttributes::new("Helvetica Neue");
        if bold {
            attributes.weight = FontWeight::BOLD;
        }
        let font = self.fonts.resolve_font(&TextStyle {
            font: vec![attributes],
            foreground: None,
        })?;
        let render_metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let top = top + ((height - render_metrics.cell_size.height as f32) / 2.).max(0.);
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
        let mut pos_x = computed.content_rect.min_x();
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
                    quad.set_position(
                        pos_x + left_offset,
                        computed.content_rect.min_y() + top_offset,
                        pos_x + left_offset + glyph_width,
                        computed.content_rect.min_y() + top_offset + glyph_height,
                    );
                    quad.set_fg_color(color);
                    quad.set_alt_color_and_mix_value(color, 0.);
                    quad.set_texture(sprite.texture_coords());
                    quad.set_hsv(None);
                    pos_x += glyph_width;
                }
                ElementCell::Glyph(glyph) => {
                    if let Some(texture) = glyph.texture.as_ref() {
                        let pos_y = computed.content_rect.min_y() + top_offset
                            - (glyph.y_offset + glyph.bearing_y).get() as f32
                            + computed.baseline;
                        if pos_x + glyph.x_advance.get() as f32 > computed.content_rect.max_x() {
                            break;
                        }
                        let glyph_x = pos_x + (glyph.x_offset + glyph.bearing_x).get() as f32;
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
        if !self.explorer.state.visible {
            return Ok(());
        }
        self.explorer.request_expanded_directories();

        let border = self.get_os_border();
        let width = self.explorer.state.width_px;
        let top = border.top.get();
        let bottom = self
            .dimensions
            .pixel_height
            .saturating_sub(border.bottom.get());
        let tree_top = top + TITLE_HEIGHT;
        let tree_bottom = bottom;
        let tree_height = tree_bottom.saturating_sub(tree_top);
        let capacity = tree_height / ROW_HEIGHT;
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
                DIVIDER_WIDTH as f32,
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

        let actions: [(ExplorerHeaderAction, &'static [Poly]); 4] = [
            (ExplorerHeaderAction::AddRoot, ADD_ROOT_ICON),
            (ExplorerHeaderAction::Reveal, REVEAL_ICON),
            (ExplorerHeaderAction::CycleFollowMode, FOLLOW_ICON),
            (ExplorerHeaderAction::ToggleHidden, HIDDEN_ICON),
        ];
        let actions_left = width.saturating_sub(actions.len() * ACTION_SIZE + 4);
        let header_text_width = actions_left.saturating_sub(TITLE_LEFT + 4);
        self.render_explorer_line(
            layers,
            "EXPLORER",
            top as f32,
            TITLE_LEFT as f32,
            header_text_width as f32,
            TITLE_HEIGHT as f32,
            TEXT,
            SIDEBAR_BG,
            false,
        )?;

        for (offset, (action, icon)) in actions.iter().enumerate() {
            let x = actions_left + offset * ACTION_SIZE;
            let y = top + (TITLE_HEIGHT - ACTION_SIZE) / 2;
            if self.explorer_item_hovered(ExplorerUiItem::Header(*action)) {
                self.filled_rectangle(
                    layers,
                    0,
                    euclid::rect(x as f32, y as f32, ACTION_SIZE as f32, ACTION_SIZE as f32),
                    HOVER_BG.to_linear_tuple_rgba(),
                )?;
            }
            self.ui_items.push(UIItem {
                x,
                y,
                width: ACTION_SIZE,
                height: ACTION_SIZE,
                item_type: UIItemType::Explorer(ExplorerUiItem::Header(*action)),
            });
            let color = match action {
                ExplorerHeaderAction::CycleFollowMode
                    if self.explorer.state.follow_mode == FollowMode::Locked =>
                {
                    ACCENT
                }
                ExplorerHeaderAction::CycleFollowMode
                    if self.explorer.state.follow_mode == FollowMode::ProjectFollow =>
                {
                    TEXT
                }
                ExplorerHeaderAction::ToggleHidden if self.explorer.state.show_hidden => ACCENT,
                _ => MUTED,
            };
            self.draw_explorer_icon(
                layers,
                icon,
                x + (ACTION_SIZE - ICON_SIZE) / 2,
                y + (ACTION_SIZE - ICON_SIZE) / 2,
                ICON_SIZE,
                color,
            )?;
        }

        let active_path = self.explorer.active_highlight_path().map(Path::to_path_buf);
        let selected_path = self.explorer.selected_path.clone();
        let start = self.explorer.rendered_start;
        let end = (start + capacity).min(self.explorer.rows.len());
        for (screen_row, row_index) in (start..end).enumerate() {
            let row = self.explorer.rows[row_index].clone();
            let y = tree_top + screen_row * ROW_HEIGHT;
            let hovered = self.explorer_item_hovered(ExplorerUiItem::Row(row_index));
            let is_active = row.path == active_path;
            let is_selected = self.explorer.focused && row.path == selected_path;
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
                    euclid::rect(0., y as f32, width as f32, ROW_HEIGHT as f32),
                    background.to_linear_tuple_rgba(),
                )?;
            }

            if row.kind == ExplorerRowKind::Root {
                self.filled_rectangle(
                    layers,
                    0,
                    euclid::rect(0., (y + ROW_HEIGHT - 1) as f32, width as f32, 1.),
                    DIVIDER.to_linear_tuple_rgba(),
                )?;
                self.draw_explorer_icon(
                    layers,
                    if row.expanded {
                        CHEVRON_DOWN
                    } else {
                        CHEVRON_RIGHT
                    },
                    2,
                    y + (ROW_HEIGHT - ICON_SIZE) / 2,
                    ICON_SIZE,
                    TEXT,
                )?;
                let max_cells = width.saturating_sub(26) / 7;
                self.render_explorer_line(
                    layers,
                    &truncate_to_width(&row.label.to_uppercase(), max_cells),
                    y as f32,
                    22.,
                    width.saturating_sub(26) as f32,
                    ROW_HEIGHT as f32,
                    TEXT,
                    background,
                    true,
                )?;
            } else {
                let depth = row.depth.saturating_sub(1);
                for guide in 0..depth {
                    let guide_x = TREE_LEFT + ICON_SIZE / 2 + guide * TREE_INDENT;
                    self.filled_rectangle(
                        layers,
                        0,
                        euclid::rect(guide_x as f32, y as f32, 1., ROW_HEIGHT as f32),
                        INDENT_GUIDE.to_linear_tuple_rgba(),
                    )?;
                }

                let twistie_x = TREE_LEFT + depth * TREE_INDENT;
                let icon_x = twistie_x + ICON_SIZE;
                let icon_y = y + (ROW_HEIGHT - ICON_SIZE) / 2;
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
                        ICON_SIZE,
                        TEXT,
                    )?;
                    self.draw_explorer_icon(layers, FOLDER_ICON, icon_x, icon_y, ICON_SIZE, ICON)?;
                } else if row.kind == ExplorerRowKind::File {
                    self.draw_explorer_icon(layers, FILE_ICON, icon_x, icon_y, ICON_SIZE, ICON)?;
                }

                let text_left = icon_x + ICON_SIZE + 2;
                let foreground = match row.kind {
                    ExplorerRowKind::Error => ERROR,
                    ExplorerRowKind::Loading | ExplorerRowKind::Truncated => MUTED,
                    _ if is_selected => RgbColor::new_8bpc(255, 255, 255),
                    _ => TEXT,
                };
                let max_cells = width.saturating_sub(text_left + 6) / 7;
                self.render_explorer_line(
                    layers,
                    &truncate_to_width(&row.label, max_cells),
                    y as f32,
                    text_left as f32,
                    width.saturating_sub(text_left + 4) as f32,
                    ROW_HEIGHT as f32,
                    foreground,
                    background,
                    false,
                )?;
            }
            self.ui_items.push(UIItem {
                x: 0,
                y,
                width,
                height: ROW_HEIGHT,
                item_type: UIItemType::Explorer(ExplorerUiItem::Row(row_index)),
            });
        }

        if self.explorer.rows.len() > capacity && capacity > 0 {
            let thumb_height = (tree_height * capacity / self.explorer.rows.len()).max(20);
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
                    width.saturating_sub(5) as f32,
                    thumb_top as f32,
                    3.,
                    thumb_height as f32,
                ),
                MUTED.to_linear_tuple_rgba(),
            )?;
        }

        self.ui_items.push(UIItem {
            x: width.saturating_sub(DIVIDER_HIT_WIDTH / 2),
            y: top,
            width: DIVIDER_HIT_WIDTH,
            height: bottom - top,
            item_type: UIItemType::Explorer(ExplorerUiItem::Divider),
        });
        Ok(())
    }

    pub fn drag_explorer_divider(&mut self, event: &MouseEvent, context: &dyn WindowOps) {
        let width = (event.coords.x.max(0) as usize).clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
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
                    let ui_item = UIItem {
                        x: self.explorer.state.width_px,
                        y: 0,
                        width: DIVIDER_HIT_WIDTH,
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
                        ExplorerHeaderAction::AddRoot => {
                            self.show_explorer_prompt(ExplorerPromptKind::AddRoot)
                        }
                        ExplorerHeaderAction::Reveal => self.reveal_active_in_explorer(),
                        ExplorerHeaderAction::CycleFollowMode => self.cycle_explorer_follow_mode(),
                        ExplorerHeaderAction::ToggleHidden => self.toggle_explorer_hidden_files(),
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
