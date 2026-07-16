use super::{TermWindow, TermWindowNotif, UIItem, UIItemType};
use crate::quad::TripleLayerQuadAllocator;
use crate::spawn::SpawnWhere;
use crate::termwindow::render::RenderScreenLineParams;
use ::window::{
    KeyCode, Modifiers, MouseCursor, MouseEvent, MouseEventKind, MousePress, Window, WindowOps,
};
use config::keyassignment::{SpawnCommand, SpawnTabDomain};
use cosmos_workspace::{
    ancestors_from_root, expand_home, DirectoryCache, ExplorerRow, ExplorerRowKind, ExplorerState,
    FollowMode, PaneContext, PaneContextRequest, ServiceResponse, WorkspaceService,
    MAX_SIDEBAR_WIDTH, MIN_SIDEBAR_WIDTH,
};
use mux::pane::CachePolicy;
use mux::renderable::{RenderableDimensions, StableCursorPosition};
use mux::tab::{SplitDirection, SplitRequest, SplitSize};
use mux::Mux;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use termwiz::cell::CellAttributes;
use termwiz::color::{ColorSpec, RgbColor};
use termwiz::input::{InputEvent, KeyCode as TermKeyCode, KeyEvent as TermKeyEvent};
use termwiz::lineedit::{Action, BasicHistory, History, LineEditor, LineEditorHost};
use termwiz::surface::{Change, Line, SEQ_ZERO};
use termwiz::terminal::Terminal;
use unicode_width::UnicodeWidthStr;
use window::color::LinearRgba;

const DIVIDER_WIDTH: usize = 1;
const DIVIDER_HIT_WIDTH: usize = 7;
const HORIZONTAL_PADDING: usize = 8;
const CONTEXT_POLL_INTERVAL: Duration = Duration::from_millis(250);
const DIRECTORY_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

const SIDEBAR_BG: RgbColor = RgbColor::new_8bpc(24, 24, 24);
const HEADER_BG: RgbColor = RgbColor::new_8bpc(31, 31, 31);
const DIVIDER: RgbColor = RgbColor::new_8bpc(47, 47, 47);
const TEXT: RgbColor = RgbColor::new_8bpc(204, 204, 204);
const MUTED: RgbColor = RgbColor::new_8bpc(133, 133, 133);
const ACTIVE_BG: RgbColor = RgbColor::new_8bpc(55, 55, 61);
const SELECTED_BG: RgbColor = RgbColor::new_8bpc(4, 57, 94);
const ACCENT: RgbColor = RgbColor::new_8bpc(0, 122, 204);
const ERROR: RgbColor = RgbColor::new_8bpc(244, 135, 113);

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
        let (state, last_error) = match ExplorerState::load(&state_path) {
            Ok(state) => (state, None),
            Err(err) => (
                ExplorerState::default(),
                Some(format!("Unable to load explorer state: {err}")),
            ),
        };
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
        let expanded = self
            .state
            .expanded
            .iter()
            .filter(|path| path.is_dir())
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
            if self.state.expanded.contains(&path) {
                directories.insert(path.clone());
            }
            if let Some(parent) = path.parent() {
                if self.state.expanded.contains(parent) {
                    directories.insert(parent.to_path_buf());
                }
            }
        }
        for directory in directories {
            self.request_directory(directory);
        }
    }

    fn apply_context(&mut self, context: PaneContext, reveal_even_if_locked: bool) -> bool {
        self.context_request_in_flight = false;
        let prior = self.active_context.clone();
        let mut changed = prior.as_ref() != Some(&context);
        let active_path = context.cwd.clone();
        self.active_context = Some(context.clone());

        if let Some(path) = active_path {
            let prior_state = self.state.clone();
            if reveal_even_if_locked && self.state.follow_mode == FollowMode::Locked {
                let root_index = self.state.ensure_root_for_path(&path);
                let root_path = self.state.roots[root_index].path.clone();
                for ancestor in ancestors_from_root(&root_path, &path) {
                    self.state.expanded.insert(ancestor);
                }
            } else if self.state.follow_mode != FollowMode::Locked {
                self.state
                    .reveal_path(&path, context.project_root.as_deref());
            }
            self.selected_path = Some(path.clone());
            self.selected_root = self.state.matching_root(&path);
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
        match self.state.follow_mode {
            FollowMode::Follow | FollowMode::Locked => context.cwd.as_deref(),
            FollowMode::ProjectFollow => context
                .project_root
                .as_deref()
                .or(context.workspace_root.as_deref())
                .or(context.cwd.as_deref()),
        }
    }

    fn rebuild_rows(&mut self, capacity: usize) {
        self.rows = self.cache.rows(&self.state);
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

    fn row(&self, index: usize) -> Option<&ExplorerRow> {
        self.rows.get(index)
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
            self.selected_root = Some(row.root_index);
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

fn row_prefix(row: &ExplorerRow) -> &'static str {
    match row.kind {
        ExplorerRowKind::Root | ExplorerRowKind::Directory if row.expanded => "⌄ ",
        ExplorerRowKind::Root | ExplorerRowKind::Directory => "› ",
        ExplorerRowKind::File => "  ",
        ExplorerRowKind::Loading => "  ",
        ExplorerRowKind::Error | ExplorerRowKind::Truncated => "! ",
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

fn process_label(process: Option<&str>) -> String {
    process
        .and_then(|process| Path::new(process).file_name())
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string())
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
        if self.explorer.state.remove_root(index).is_some() {
            self.explorer.selected_path = None;
            self.explorer.selected_root = None;
            self.explorer.cache.clear();
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
                    return;
                }
                let index = self.explorer.state.add_root(path.clone());
                self.explorer.state.expanded.insert(path.clone());
                self.explorer.selected_path = Some(path.clone());
                self.explorer.selected_root = Some(index);
                self.explorer.request_directory(path);
                self.explorer.persist();
            }
            ExplorerPromptKind::RenameRoot(index) => {
                self.explorer.state.rename_root(index, value);
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
        foreground: RgbColor,
        background: RgbColor,
    ) -> anyhow::Result<()> {
        let mut attributes = CellAttributes::default();
        attributes
            .set_foreground(ColorSpec::from(foreground))
            .set_background(ColorSpec::from(background));
        let line = Line::from_text(text, &attributes, SEQ_ZERO, None);
        let palette = self.palette().clone();
        let gl_state = self.render_state.as_ref().unwrap();
        let white_space = gl_state.util_sprites.white_space.texture_coords();
        let filled_box = gl_state.util_sprites.filled_box.texture_coords();
        let cell_width = self.render_metrics.cell_size.width as usize;
        let cols = (width as usize / cell_width).max(1);
        self.render_screen_line(
            RenderScreenLineParams {
                top_pixel_y: top,
                left_pixel_x: left,
                pixel_width: width,
                stable_line_idx: None,
                line: &line,
                selection: 0..0,
                cursor: &StableCursorPosition::default(),
                palette: &palette,
                dims: &RenderableDimensions {
                    cols,
                    physical_top: 0,
                    scrollback_rows: 0,
                    scrollback_top: 0,
                    viewport_rows: 1,
                    dpi: self.terminal_size.dpi,
                    pixel_height: self.render_metrics.cell_size.height as usize,
                    pixel_width: width as usize,
                    reverse_video: false,
                },
                config: &self.config,
                cursor_border_color: LinearRgba::default(),
                foreground: foreground.to_linear_tuple_rgba(),
                pane: None,
                is_active: true,
                selection_fg: LinearRgba::default(),
                selection_bg: LinearRgba::default(),
                cursor_fg: LinearRgba::default(),
                cursor_bg: LinearRgba::default(),
                cursor_is_default_color: true,
                white_space,
                filled_box,
                window_is_transparent: false,
                default_bg: background.to_linear_tuple_rgba(),
                style: None,
                font: None,
                use_pixel_positioning: self.config.experimental_pixel_positioning,
                render_metrics: self.render_metrics,
                shape_key: None,
                password_input: false,
            },
            layers,
        )?;
        Ok(())
    }

    pub fn paint_explorer(&mut self, layers: &mut TripleLayerQuadAllocator) -> anyhow::Result<()> {
        if !self.explorer.state.visible {
            return Ok(());
        }
        self.explorer.request_expanded_directories();

        let border = self.get_os_border();
        let width = self.explorer.state.width_px;
        let cell_height = self.render_metrics.cell_size.height as usize;
        let header_height = self
            .tab_bar_pixel_height()
            .unwrap_or(cell_height as f32)
            .max(cell_height as f32) as usize;
        let top = border.top.get();
        let bottom = self
            .dimensions
            .pixel_height
            .saturating_sub(border.bottom.get());
        let status_rows = 4usize;
        let status_height = status_rows * cell_height;
        let tree_top = top + header_height;
        let tree_bottom = bottom.saturating_sub(status_height);
        let tree_height = tree_bottom.saturating_sub(tree_top);
        let capacity = tree_height / cell_height;
        self.explorer.rebuild_rows(capacity);

        self.filled_rectangle(
            layers,
            0,
            euclid::rect(0., top as f32, width as f32, (bottom - top) as f32),
            SIDEBAR_BG.to_linear_tuple_rgba(),
        )?;
        self.filled_rectangle(
            layers,
            0,
            euclid::rect(0., top as f32, width as f32, header_height as f32),
            HEADER_BG.to_linear_tuple_rgba(),
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

        let actions = [
            (ExplorerHeaderAction::AddRoot, "+"),
            (ExplorerHeaderAction::Reveal, "◎"),
            (ExplorerHeaderAction::CycleFollowMode, "⇄"),
            (
                ExplorerHeaderAction::ToggleHidden,
                if self.explorer.state.show_hidden {
                    "•"
                } else {
                    "◦"
                },
            ),
        ];
        let action_width = cell_height.max(22);
        let header_text_width = width
            .saturating_sub(actions.len() * action_width)
            .saturating_sub(HORIZONTAL_PADDING * 2);
        let header_cells = header_text_width / self.render_metrics.cell_size.width as usize;
        let follow_label = self.explorer.state.follow_mode.label();
        let full_header = format!("EXPLORER  {follow_label}");
        let compact_header = format!("EXP  {follow_label}");
        let header = if full_header.chars().count() <= header_cells {
            full_header
        } else if compact_header.chars().count() <= header_cells {
            compact_header
        } else {
            truncate_to_width(follow_label, header_cells)
        };
        self.render_explorer_line(
            layers,
            &header,
            top as f32,
            HORIZONTAL_PADDING as f32,
            header_text_width as f32,
            TEXT,
            HEADER_BG,
        )?;

        for (offset, (action, label)) in actions.iter().enumerate() {
            let x = width.saturating_sub((actions.len() - offset) * action_width);
            self.ui_items.push(UIItem {
                x,
                y: top,
                width: action_width,
                height: header_height,
                item_type: UIItemType::Explorer(ExplorerUiItem::Header(*action)),
            });
            self.render_explorer_line(
                layers,
                label,
                top as f32,
                (x + action_width / 3) as f32,
                action_width as f32,
                MUTED,
                HEADER_BG,
            )?;
        }

        let active_path = self.explorer.active_highlight_path().map(Path::to_path_buf);
        let selected_path = self.explorer.selected_path.clone();
        let start = self.explorer.rendered_start;
        let end = (start + capacity).min(self.explorer.rows.len());
        for (screen_row, row_index) in (start..end).enumerate() {
            let row = self.explorer.rows[row_index].clone();
            let y = tree_top + screen_row * cell_height;
            let is_active = row.path == active_path;
            let is_selected = self.explorer.focused && row.path == selected_path;
            let background = if is_selected {
                SELECTED_BG
            } else if is_active {
                ACTIVE_BG
            } else {
                SIDEBAR_BG
            };
            if background != SIDEBAR_BG {
                self.filled_rectangle(
                    layers,
                    0,
                    euclid::rect(0., y as f32, width as f32, cell_height as f32),
                    background.to_linear_tuple_rgba(),
                )?;
            }
            if is_active {
                self.filled_rectangle(
                    layers,
                    2,
                    euclid::rect(0., y as f32, 2., cell_height as f32),
                    ACCENT.to_linear_tuple_rgba(),
                )?;
            }
            let indent = "  ".repeat(row.depth);
            let text = format!("{indent}{}{}", row_prefix(&row), row.label);
            let max_cells = width.saturating_sub(HORIZONTAL_PADDING * 2)
                / self.render_metrics.cell_size.width as usize;
            let text = truncate_to_width(&text, max_cells);
            let foreground = match row.kind {
                ExplorerRowKind::Error => ERROR,
                ExplorerRowKind::Loading | ExplorerRowKind::Truncated => MUTED,
                _ => TEXT,
            };
            self.render_explorer_line(
                layers,
                &text,
                y as f32,
                HORIZONTAL_PADDING as f32,
                width.saturating_sub(HORIZONTAL_PADDING * 2) as f32,
                foreground,
                background,
            )?;
            self.ui_items.push(UIItem {
                x: 0,
                y,
                width,
                height: cell_height,
                item_type: UIItemType::Explorer(ExplorerUiItem::Row(row_index)),
            });
        }

        self.filled_rectangle(
            layers,
            2,
            euclid::rect(0., tree_bottom as f32, width as f32, DIVIDER_WIDTH as f32),
            DIVIDER.to_linear_tuple_rgba(),
        )?;
        let context_lines = if let Some(context) = &self.explorer.active_context {
            vec![
                format!(
                    "Pane {} · {} · {}",
                    context.pane_id,
                    process_label(context.foreground_process.as_deref()),
                    context.source.label()
                ),
                context
                    .cwd
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "Current directory unavailable".to_string()),
                context
                    .workspace_root
                    .as_ref()
                    .map(|path| format!("Workspace: {}", path.display()))
                    .unwrap_or_else(|| "Workspace: unresolved".to_string()),
                self.explorer
                    .last_error
                    .clone()
                    .or_else(|| context.error.clone())
                    .unwrap_or_else(|| "↑↓ navigate · ←→ expand · ⌘↵ new tab".to_string()),
            ]
        } else {
            vec![
                "Waiting for pane context…".to_string(),
                String::new(),
                String::new(),
                "A add root · F/P/L follow · R reveal".to_string(),
            ]
        };
        for (index, text) in context_lines.iter().enumerate() {
            let y = tree_bottom + index * cell_height;
            let max_cells = width.saturating_sub(HORIZONTAL_PADDING * 2)
                / self.render_metrics.cell_size.width as usize;
            self.render_explorer_line(
                layers,
                &truncate_to_width(text, max_cells),
                y as f32,
                HORIZONTAL_PADDING as f32,
                width.saturating_sub(HORIZONTAL_PADDING * 2) as f32,
                if index == 0 { TEXT } else { MUTED },
                SIDEBAR_BG,
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
