use super::{TermWindow, TermWindowNotif, UIItem, UIItemType};
use crate::customglyph::{BlockAlpha, BlockCoord, Poly, PolyCommand, PolyStyle};
use crate::quad::{QuadTrait, TripleLayerQuadAllocator, TripleLayerQuadAllocatorTrait};
use crate::spawn::SpawnWhere;
use crate::termwindow::box_model::{Element, ElementCell, ElementContent, LayoutContext};
use crate::utilsprites::RenderMetrics;
use ::window::{
    KeyCode, Modifiers, MouseCursor, MouseEvent, MouseEventKind, MousePress, Window, WindowOps,
};
use config::keyassignment::{KeyAssignment, SpawnCommand, SpawnTabDomain};
use config::{DimensionContext, FontAttributes, FontWeight, TextStyle};
use cosmos_workspace::{
    classify_codex_prompt, expand_home, process_is_codex, process_is_tmux, CodexPromptAuditEvent,
    CodexPromptAutomationMode, CodexPromptMatch, DirectoryCache, ExplorerRow, ExplorerRowKind,
    ExplorerState, FileRequest, FileRevision, FollowMode, GitFileStatus, PaneContext,
    PaneContextRequest, ServiceResponse, TmuxCodexPromptChoiceResult, TmuxCodexPromptScan,
    TmuxPaneGeometry, WorkspaceRoot, WorkspaceService, WorkspaceStatusSnapshot, MAX_SIDEBAR_WIDTH,
    MIN_SIDEBAR_WIDTH,
};
use mux::pane::{CachePolicy, PaneId};
use mux::tab::{SplitDirection, SplitRequest, SplitSize};
use mux::Mux;
use ordered_float::NotNan;
use pulldown_cmark::{Event as MarkdownEvent, Options as MarkdownOptions, Parser, Tag};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};
use termwiz::cell::CellAttributes;
use termwiz::color::RgbColor;
use termwiz::input::{InputEvent, KeyCode as TermKeyCode, KeyEvent as TermKeyEvent};
use termwiz::lineedit::{Action, BasicHistory, History, LineEditor, LineEditorHost};
use termwiz::surface::Change;
use termwiz::terminal::Terminal;
use unicode_width::UnicodeWidthStr;
use wezterm_term::{KeyCode as PaneKeyCode, KeyModifiers as PaneKeyModifiers};

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
const FILE_HEADER_HEIGHT: usize = 38;
const FILE_CONTENT_PADDING: usize = 24;
const FILE_BODY_FONT_LOGICAL_SIZE: f64 = 15.0;
const FILE_CODE_FONT_LOGICAL_SIZE: f64 = 14.0;
#[cfg(target_os = "macos")]
const EXPLORER_UI_FONT_FAMILY: &str = "System Font";
#[cfg(not(target_os = "macos"))]
const EXPLORER_UI_FONT_FAMILY: &str = "Helvetica Neue";
const VIRTUAL_ROOT_INDEX: usize = usize::MAX;
const SERVICE_POLL_INTERVAL: Duration = Duration::from_millis(50);
const SERVICE_IDLE_POLL_INTERVAL: Duration = Duration::from_millis(500);
const CONTEXT_POLL_INTERVAL: Duration = Duration::from_millis(500);
const DIRECTORY_REFRESH_INTERVAL: Duration = Duration::from_secs(30);
const GIT_STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(30);
const WORKSPACE_STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const CODEX_PROMPT_DEBOUNCE: Duration = Duration::from_millis(200);
const CODEX_PROMPT_MANUAL_INPUT_PAUSE: Duration = Duration::from_secs(2);
const CODEX_PROMPT_TMUX_SCAN_INTERVAL: Duration = Duration::from_secs(2);

fn logical_to_physical(value: usize, dpi: usize) -> usize {
    if value == 0 {
        return 0;
    }
    ((value as f64 * dpi.max(1) as f64 / 72.).round() as usize).max(1)
}

fn physical_to_logical(value: usize, dpi: usize) -> usize {
    ((value as f64 * 72. / dpi.max(1) as f64).round() as usize).max(1)
}

fn tmux_key_matches_event(binding: &str, key: &KeyCode, modifiers: Modifiers) -> bool {
    let parts = binding.split('-').collect::<Vec<_>>();
    let Some(key_name) = parts.last().copied().filter(|key| !key.is_empty()) else {
        return false;
    };
    let mut expected = Modifiers::NONE;
    for modifier in &parts[..parts.len().saturating_sub(1)] {
        match *modifier {
            "C" => expected |= Modifiers::CTRL,
            "M" => expected |= Modifiers::ALT,
            "S" => expected |= Modifiers::SHIFT,
            _ => return false,
        }
    }
    if modifiers.remove_positional_mods() != expected {
        return false;
    }

    match key_name {
        "BSpace" => matches!(key, KeyCode::Char('\u{8}' | '\u{7f}')),
        "DC" | "Delete" => matches!(key, KeyCode::Char('\u{7f}')),
        "Space" => matches!(key, KeyCode::Char(' ')),
        "Enter" => matches!(key, KeyCode::Char('\r')),
        "Escape" => matches!(key, KeyCode::Char('\u{1b}')),
        "Tab" => matches!(key, KeyCode::Char('\t')),
        "Up" => matches!(key, KeyCode::UpArrow),
        "Down" => matches!(key, KeyCode::DownArrow),
        "Left" => matches!(key, KeyCode::LeftArrow),
        "Right" => matches!(key, KeyCode::RightArrow),
        "Home" => matches!(key, KeyCode::Home),
        "End" => matches!(key, KeyCode::End),
        "PPage" => matches!(key, KeyCode::PageUp),
        "NPage" => matches!(key, KeyCode::PageDown),
        name => {
            let mut chars = name.chars();
            let Some(expected_char) = chars.next() else {
                return false;
            };
            chars.next().is_none()
                && matches!(
                    key,
                    KeyCode::Char(actual) if actual.eq_ignore_ascii_case(&expected_char)
                )
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExplorerKeyboardAction {
    Move(isize),
    Collapse,
    Expand,
    Activate,
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FilePreviewAction {
    ScrollVertical(isize),
    ScrollHorizontal(isize),
    Exit,
}

fn explorer_keyboard_action(key: &KeyCode, modifiers: Modifiers) -> Option<ExplorerKeyboardAction> {
    match (key, modifiers.remove_positional_mods()) {
        (KeyCode::Char('w'), Modifiers::NONE) => Some(ExplorerKeyboardAction::Move(-1)),
        (KeyCode::Char('s'), Modifiers::NONE) => Some(ExplorerKeyboardAction::Move(1)),
        // macOS can fold Shift into the uppercase character before the
        // non-binding input pass. Accept both that normalized form and the
        // explicit Shift modifier so physical Shift+W/S always moves five.
        (KeyCode::Char('W'), Modifiers::NONE) | (KeyCode::Char('w' | 'W'), Modifiers::SHIFT) => {
            Some(ExplorerKeyboardAction::Move(-5))
        }
        (KeyCode::Char('S'), Modifiers::NONE) | (KeyCode::Char('s' | 'S'), Modifiers::SHIFT) => {
            Some(ExplorerKeyboardAction::Move(5))
        }
        (KeyCode::UpArrow, Modifiers::NONE) => Some(ExplorerKeyboardAction::Move(-1)),
        (KeyCode::DownArrow, Modifiers::NONE) => Some(ExplorerKeyboardAction::Move(1)),
        (KeyCode::LeftArrow, Modifiers::NONE) | (KeyCode::Char('a' | 'A'), Modifiers::NONE) => {
            Some(ExplorerKeyboardAction::Collapse)
        }
        (KeyCode::RightArrow, Modifiers::NONE) | (KeyCode::Char('d' | 'D'), Modifiers::NONE) => {
            Some(ExplorerKeyboardAction::Expand)
        }
        (KeyCode::Char('\r'), Modifiers::NONE) => Some(ExplorerKeyboardAction::Activate),
        (KeyCode::Char('\u{1b}'), Modifiers::NONE) => Some(ExplorerKeyboardAction::Exit),
        _ => None,
    }
}

fn file_preview_action(key: &KeyCode, modifiers: Modifiers) -> Option<FilePreviewAction> {
    match (key, modifiers.remove_positional_mods()) {
        (KeyCode::Char('w'), Modifiers::NONE) | (KeyCode::UpArrow, Modifiers::NONE) => {
            Some(FilePreviewAction::ScrollVertical(-1))
        }
        (KeyCode::Char('s'), Modifiers::NONE) | (KeyCode::DownArrow, Modifiers::NONE) => {
            Some(FilePreviewAction::ScrollVertical(1))
        }
        // The macOS text-input path can normalize a physical Shift+letter to
        // uppercase with no Shift bit. Accept both forms for the jump keys.
        (KeyCode::Char('W'), Modifiers::NONE)
        | (KeyCode::Char('w' | 'W'), Modifiers::SHIFT)
        | (KeyCode::PageUp, Modifiers::NONE) => Some(FilePreviewAction::ScrollVertical(-5)),
        (KeyCode::Char('S'), Modifiers::NONE)
        | (KeyCode::Char('s' | 'S'), Modifiers::SHIFT)
        | (KeyCode::PageDown, Modifiers::NONE) => Some(FilePreviewAction::ScrollVertical(5)),
        (KeyCode::Char('a'), Modifiers::NONE) | (KeyCode::LeftArrow, Modifiers::NONE) => {
            Some(FilePreviewAction::ScrollHorizontal(-1))
        }
        (KeyCode::Char('d'), Modifiers::NONE) | (KeyCode::RightArrow, Modifiers::NONE) => {
            Some(FilePreviewAction::ScrollHorizontal(1))
        }
        (KeyCode::Char('A'), Modifiers::NONE) | (KeyCode::Char('a' | 'A'), Modifiers::SHIFT) => {
            Some(FilePreviewAction::ScrollHorizontal(-8))
        }
        (KeyCode::Char('D'), Modifiers::NONE) | (KeyCode::Char('d' | 'D'), Modifiers::SHIFT) => {
            Some(FilePreviewAction::ScrollHorizontal(8))
        }
        (KeyCode::Char('\u{1b}'), Modifiers::NONE) => Some(FilePreviewAction::Exit),
        _ => None,
    }
}

fn apply_preview_scroll(value: usize, delta: isize) -> usize {
    if delta < 0 {
        value.saturating_sub((-delta) as usize)
    } else {
        value.saturating_add(delta as usize)
    }
}

fn is_tmux_explorer_toggle_key(key: &KeyCode, modifiers: Modifiers) -> bool {
    matches!(
        (key, modifiers.remove_positional_mods()),
        (KeyCode::Char('0'), Modifiers::NONE)
    )
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
const FILE_BG: RgbColor = RgbColor::new_8bpc(30, 30, 30);
const FILE_HEADER_BG: RgbColor = RgbColor::new_8bpc(24, 24, 24);
const FILE_BORDER: RgbColor = RgbColor::new_8bpc(63, 63, 70);
const FILE_LINK: RgbColor = RgbColor::new_8bpc(78, 148, 206);
const FILE_CODE_BG: RgbColor = RgbColor::new_8bpc(37, 37, 38);

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileWorkspaceUiItem {
    Surface,
    TerminalTab,
    EditToggle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerPromptKind {
    AddRoot,
    RenameRoot(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileWorkspaceMode {
    Terminal,
    View,
    Edit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DocumentLineKind {
    Body,
    Heading(u8),
    Code,
    Quote,
    List,
    Rule,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DocumentLine {
    text: String,
    kind: DocumentLineKind,
}

#[derive(Debug)]
struct FileWorkspaceUi {
    mode: FileWorkspaceMode,
    resume_mode: FileWorkspaceMode,
    owner_pane_id: Option<usize>,
    owner_tmux_pane_id: Option<String>,
    owner_tmux_window_id: Option<String>,
    owner_tmux_geometry: Option<TmuxPaneGeometry>,
    owner_root: Option<PathBuf>,
    tmux_prefix_pending: Option<(KeyCode, Modifiers)>,
    explorer_keyboard_mode: bool,
    active_path: Option<PathBuf>,
    content: String,
    document_lines: Vec<DocumentLine>,
    wrapped_document_lines: Vec<DocumentLine>,
    wrap_columns: usize,
    dirty: bool,
    revision: Option<FileRevision>,
    cursor: usize,
    scroll: usize,
    horizontal_scroll: usize,
    request_id: u64,
    pending_request: Option<u64>,
    error: Option<String>,
}

impl Default for FileWorkspaceUi {
    fn default() -> Self {
        Self {
            mode: FileWorkspaceMode::Terminal,
            resume_mode: FileWorkspaceMode::View,
            owner_pane_id: None,
            owner_tmux_pane_id: None,
            owner_tmux_window_id: None,
            owner_tmux_geometry: None,
            owner_root: None,
            tmux_prefix_pending: None,
            explorer_keyboard_mode: false,
            active_path: None,
            content: String::new(),
            document_lines: vec![],
            wrapped_document_lines: vec![],
            wrap_columns: 0,
            dirty: false,
            revision: None,
            cursor: 0,
            scroll: 0,
            horizontal_scroll: 0,
            request_id: 0,
            pending_request: None,
            error: None,
        }
    }
}

impl FileWorkspaceUi {
    fn visible(&self) -> bool {
        self.mode != FileWorkspaceMode::Terminal
    }

    fn next_request_id(&mut self) -> u64 {
        self.request_id = self.request_id.wrapping_add(1).max(1);
        self.pending_request = Some(self.request_id);
        self.request_id
    }

    fn reset_for_context(
        &mut self,
        pane_id: usize,
        tmux_pane_id: Option<String>,
        tmux_window_id: Option<String>,
        tmux_geometry: Option<TmuxPaneGeometry>,
        root: PathBuf,
    ) {
        self.request_id = self.request_id.wrapping_add(1).max(1);
        self.pending_request = None;
        self.mode = FileWorkspaceMode::View;
        self.resume_mode = FileWorkspaceMode::View;
        self.owner_pane_id = Some(pane_id);
        self.owner_tmux_pane_id = tmux_pane_id;
        self.owner_tmux_window_id = tmux_window_id;
        self.owner_tmux_geometry = tmux_geometry;
        self.owner_root = Some(root);
        self.tmux_prefix_pending = None;
        self.explorer_keyboard_mode = false;
        self.active_path = None;
        self.content.clear();
        self.document_lines.clear();
        self.wrapped_document_lines.clear();
        self.wrap_columns = 0;
        self.dirty = false;
        self.revision = None;
        self.cursor = 0;
        self.scroll = 0;
        self.horizontal_scroll = 0;
        self.error = None;
    }

    fn is_markdown(&self) -> bool {
        self.active_path
            .as_deref()
            .and_then(Path::extension)
            .map(|extension| {
                matches!(
                    extension.to_string_lossy().to_ascii_lowercase().as_str(),
                    "md" | "mdx" | "markdown"
                )
            })
            .unwrap_or(false)
    }

    fn rebuild_document(&mut self) {
        self.document_lines = if self.is_markdown() {
            markdown_document_lines(&self.content)
        } else {
            self.content
                .lines()
                .map(|line| DocumentLine {
                    text: line.to_string(),
                    kind: DocumentLineKind::Code,
                })
                .collect()
        };
        if self.document_lines.is_empty() {
            self.document_lines.push(DocumentLine {
                text: String::new(),
                kind: if self.is_markdown() {
                    DocumentLineKind::Body
                } else {
                    DocumentLineKind::Code
                },
            });
        }
        self.wrapped_document_lines.clear();
        self.wrap_columns = 0;
    }

    fn ensure_wrapped_document(&mut self, columns: usize) {
        let columns = columns.max(24);
        if self.wrap_columns == columns && !self.wrapped_document_lines.is_empty() {
            return;
        }
        self.wrap_columns = columns;
        self.wrapped_document_lines.clear();
        for line in &self.document_lines {
            if line.text.is_empty()
                || matches!(line.kind, DocumentLineKind::Code | DocumentLineKind::Rule)
            {
                self.wrapped_document_lines.push(line.clone());
                continue;
            }
            let width = match line.kind {
                DocumentLineKind::List | DocumentLineKind::Quote => columns.saturating_sub(4),
                _ => columns,
            }
            .max(16);
            let wrapped = textwrap::wrap(&line.text, width);
            if wrapped.is_empty() {
                self.wrapped_document_lines.push(line.clone());
            } else {
                self.wrapped_document_lines
                    .extend(wrapped.into_iter().map(|text| DocumentLine {
                        text: text.into_owned(),
                        kind: line.kind,
                    }));
            }
        }
    }

    fn trace_test_state(&self, event: &str) {
        let path = match std::env::var_os("COSMOS_TERM_FILE_WORKSPACE_TRACE") {
            Some(path) => PathBuf::from(path),
            None => return,
        };
        let mode = match self.mode {
            FileWorkspaceMode::Terminal => "terminal",
            FileWorkspaceMode::View => "view",
            FileWorkspaceMode::Edit => "edit",
        };
        let snapshot = serde_json::json!({
            "event": event,
            "mode": mode,
            "path": self.active_path,
            "content_bytes": self.content.len(),
            "document_lines": self.document_lines.len(),
            "scroll": self.scroll,
            "horizontal_scroll": self.horizontal_scroll,
            "dirty": self.dirty,
            "pending": self.pending_request,
            "error": self.error,
            "explorer_keyboard_mode": self.explorer_keyboard_mode,
        });
        if let Ok(data) = serde_json::to_vec_pretty(&snapshot) {
            let _ = std::fs::write(path, data);
        }
    }
}

#[derive(Clone)]
struct CodexPromptCandidate {
    prompt: CodexPromptMatch,
    first_seen: Instant,
}

#[derive(Clone)]
struct TmuxPromptScanRequest {
    outer_pane_id: PaneId,
    tty_name: String,
    tmux_executable: String,
}

struct CodexPromptAutomationUi {
    native_pending: HashMap<PaneId, Instant>,
    native_candidates: HashMap<PaneId, CodexPromptCandidate>,
    native_handled: HashMap<PaneId, CodexPromptMatch>,
    recent_manual_input: HashMap<PaneId, Instant>,
    tmux_scan_queue: VecDeque<TmuxPromptScanRequest>,
    tmux_scan_in_flight: Option<TmuxPromptScanRequest>,
    tmux_candidates: HashMap<String, CodexPromptCandidate>,
    tmux_handled: HashMap<String, CodexPromptMatch>,
    tmux_choices_pending: HashMap<String, CodexPromptMatch>,
    last_tmux_scan: Instant,
    choices_sent: usize,
}

impl Default for CodexPromptAutomationUi {
    fn default() -> Self {
        Self {
            native_pending: HashMap::new(),
            native_candidates: HashMap::new(),
            native_handled: HashMap::new(),
            recent_manual_input: HashMap::new(),
            tmux_scan_queue: VecDeque::new(),
            tmux_scan_in_flight: None,
            tmux_candidates: HashMap::new(),
            tmux_handled: HashMap::new(),
            tmux_choices_pending: HashMap::new(),
            last_tmux_scan: Instant::now()
                .checked_sub(CODEX_PROMPT_TMUX_SCAN_INTERVAL)
                .unwrap_or_else(Instant::now),
            choices_sent: 0,
        }
    }
}

fn shared_codex_prompt_deduplication() -> &'static Mutex<HashMap<String, u64>> {
    static DEDUPLICATION: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    DEDUPLICATION.get_or_init(|| Mutex::new(HashMap::new()))
}

fn reserve_codex_prompt(target_id: &str, fingerprint: u64) -> bool {
    let Ok(mut handled) = shared_codex_prompt_deduplication().lock() else {
        return false;
    };
    match handled.get(target_id) {
        Some(_) => false,
        None => {
            handled.insert(target_id.to_string(), fingerprint);
            true
        }
    }
}

fn clear_codex_prompt_reservation(target_id: &str) {
    if let Ok(mut handled) = shared_codex_prompt_deduplication().lock() {
        handled.remove(target_id);
    }
}

fn opaque_native_prompt_target(mux_window_id: usize, pane_id: PaneId) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in mux_window_id
        .to_le_bytes()
        .iter()
        .copied()
        .chain(pane_id.to_le_bytes().iter().copied())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub struct ExplorerUi {
    pub state: ExplorerState,
    state_path: PathBuf,
    service: WorkspaceService,
    cache: DirectoryCache,
    pending_directories: HashSet<PathBuf>,
    watched_directories: HashSet<PathBuf>,
    rows: Vec<ExplorerRow>,
    rows_dirty: bool,
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
    workspace_status: WorkspaceStatusSnapshot,
    workspace_status_request_in_flight: bool,
    last_context_request: Instant,
    last_directory_refresh: Instant,
    last_git_status_refresh: Instant,
    last_workspace_status_refresh: Instant,
    tick_scheduled: bool,
    last_error: Option<String>,
    file_workspace: FileWorkspaceUi,
    codex_prompt_automation: CodexPromptAutomationUi,
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
        let service = WorkspaceService::new();
        service.set_codex_prompt_actions_enabled(
            state.codex_prompt_automation_mode == CodexPromptAutomationMode::Active,
        );
        Self {
            state,
            state_path,
            service,
            cache: DirectoryCache::default(),
            pending_directories: HashSet::new(),
            watched_directories: HashSet::new(),
            rows: vec![],
            rows_dirty: true,
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
            workspace_status: WorkspaceStatusSnapshot::default(),
            workspace_status_request_in_flight: false,
            last_context_request: Instant::now()
                .checked_sub(CONTEXT_POLL_INTERVAL)
                .unwrap_or_else(Instant::now),
            last_directory_refresh: Instant::now(),
            last_git_status_refresh: Instant::now()
                .checked_sub(GIT_STATUS_REFRESH_INTERVAL)
                .unwrap_or_else(Instant::now),
            last_workspace_status_refresh: Instant::now()
                .checked_sub(WORKSPACE_STATUS_REFRESH_INTERVAL)
                .unwrap_or_else(Instant::now),
            tick_scheduled: false,
            last_error,
            file_workspace: FileWorkspaceUi::default(),
            codex_prompt_automation: CodexPromptAutomationUi::default(),
        }
    }

    pub fn total_width(&self, dpi: usize) -> usize {
        logical_to_physical(self.state.width_px + DIVIDER_WIDTH, dpi)
    }

    fn persist(&mut self) {
        if let Err(err) = self.state.save(&self.state_path) {
            self.last_error = Some(format!("Unable to save explorer state: {err}"));
            self.rows_dirty = true;
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

    fn request_workspace_status(&mut self) {
        if self.workspace_status_request_in_flight {
            return;
        }
        self.workspace_status_request_in_flight = true;
        self.service.workspace_status(self.codex_home.clone());
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
        if changed {
            self.display_root = Some(root);
            self.scroll = 0;
            self.selected_path = Some(path.clone());
            self.git_statuses.clear();
            self.rows_dirty = true;
        }
        self.selected_root = self.exact_root_index(&path);
        if changed || expanded {
            self.rows_dirty = true;
            self.ensure_expanded_directories();
        }
        if changed {
            self.request_git_status();
        }
        changed || expanded
    }

    fn apply_context(&mut self, context: PaneContext, reveal_active: bool) -> bool {
        self.context_request_in_flight = false;
        let mut changed = false;
        self.active_context = Some(context.clone());

        if let Some(cwd) = context.cwd.clone() {
            // Cosmos intentionally differs from a multi-root editor workspace:
            // the explorer is a permanent view of the active pane's exact
            // working directory. Serialized follow modes are retained only for
            // backward-compatible state loading.
            let follow_mode_changed = self.state.follow_mode != FollowMode::Follow;
            let was_expanded = self.state.expanded.contains(&cwd);
            self.state.follow_mode = FollowMode::Follow;
            let root_changed = self.set_display_root(cwd.clone(), reveal_active);
            changed |= root_changed;
            if (root_changed || !self.focused)
                && self.selected_path.as_deref() != Some(cwd.as_path())
            {
                self.selected_path = Some(cwd);
                self.rows_dirty = true;
                changed = true;
            }
            let expansion_changed = !was_expanded
                && self
                    .display_root
                    .as_ref()
                    .map(|root| self.state.expanded.contains(&root.path))
                    .unwrap_or(false);
            if follow_mode_changed || expansion_changed {
                self.persist();
                changed = true;
            }
        } else if let Some(error) = &context.error {
            if self.last_error.as_ref() != Some(error) {
                self.last_error = Some(error.clone());
                self.rows_dirty = true;
                changed = true;
            }
        }
        changed
    }

    fn active_highlight_path(&self) -> Option<&Path> {
        let context = self.active_context.as_ref()?;
        let cwd = context.cwd.as_deref()?;
        self.is_in_display_scope(cwd).then_some(cwd)
    }

    fn rebuild_rows(&mut self, capacity: usize) {
        if !self.rows_dirty && self.rendered_capacity == capacity {
            self.scroll = self
                .scroll
                .min(self.rows.len().saturating_sub(capacity.max(1)));
            self.rendered_start = self.scroll;
            return;
        }
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
        self.rows_dirty = false;
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
            self.rows_dirty = true;
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
        self.rows_dirty = true;
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

fn markdown_document_lines(markdown: &str) -> Vec<DocumentLine> {
    let parser = Parser::new_ext(markdown, MarkdownOptions::all());
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut kind = DocumentLineKind::Body;
    let mut list_depth = 0usize;
    let mut link_destination: Option<String> = None;
    let mut in_table = false;

    let flush = |lines: &mut Vec<DocumentLine>, current: &mut String, kind: DocumentLineKind| {
        if !current.is_empty() || matches!(kind, DocumentLineKind::Rule) {
            lines.push(DocumentLine {
                text: std::mem::take(current),
                kind,
            });
        }
    };

    for event in parser {
        match event {
            MarkdownEvent::Start(Tag::Heading(level, _, _)) => {
                flush(&mut lines, &mut current, kind);
                kind = DocumentLineKind::Heading(level as u8);
            }
            MarkdownEvent::End(Tag::Heading(_, _, _)) => {
                flush(&mut lines, &mut current, kind);
                kind = DocumentLineKind::Body;
            }
            MarkdownEvent::Start(Tag::CodeBlock(_)) => {
                flush(&mut lines, &mut current, kind);
                kind = DocumentLineKind::Code;
            }
            MarkdownEvent::End(Tag::CodeBlock(_)) => {
                flush(&mut lines, &mut current, kind);
                kind = DocumentLineKind::Body;
            }
            MarkdownEvent::Start(Tag::BlockQuote) => {
                flush(&mut lines, &mut current, kind);
                kind = DocumentLineKind::Quote;
                current.push_str("│ ");
            }
            MarkdownEvent::End(Tag::BlockQuote) => {
                flush(&mut lines, &mut current, kind);
                kind = DocumentLineKind::Body;
            }
            MarkdownEvent::Start(Tag::List(_)) => list_depth += 1,
            MarkdownEvent::End(Tag::List(_)) => list_depth = list_depth.saturating_sub(1),
            MarkdownEvent::Start(Tag::Table(_)) => {
                flush(&mut lines, &mut current, kind);
                in_table = true;
                kind = DocumentLineKind::Code;
            }
            MarkdownEvent::End(Tag::Table(_)) => {
                flush(&mut lines, &mut current, kind);
                in_table = false;
                kind = DocumentLineKind::Body;
                lines.push(DocumentLine {
                    text: String::new(),
                    kind,
                });
            }
            MarkdownEvent::Start(Tag::TableHead) | MarkdownEvent::Start(Tag::TableRow) => {
                flush(&mut lines, &mut current, kind);
            }
            MarkdownEvent::End(Tag::TableHead) | MarkdownEvent::End(Tag::TableRow) => {
                flush(&mut lines, &mut current, DocumentLineKind::Code);
            }
            MarkdownEvent::Start(Tag::TableCell) => {
                if !current.is_empty() {
                    current.push_str("  │  ");
                }
            }
            MarkdownEvent::End(Tag::TableCell) => {}
            MarkdownEvent::Start(Tag::Item) => {
                flush(&mut lines, &mut current, kind);
                kind = DocumentLineKind::List;
                current.push_str(&"  ".repeat(list_depth.saturating_sub(1)));
                current.push_str("• ");
            }
            MarkdownEvent::End(Tag::Item) => {
                flush(&mut lines, &mut current, kind);
                kind = DocumentLineKind::Body;
            }
            MarkdownEvent::Start(Tag::Link(_, destination, _)) => {
                link_destination = Some(destination.to_string());
            }
            MarkdownEvent::End(Tag::Link(_, _, _)) => {
                if let Some(destination) = link_destination.take() {
                    current.push_str("  ‹");
                    current.push_str(&destination);
                    current.push('›');
                }
            }
            MarkdownEvent::Text(text) => current.push_str(&text),
            MarkdownEvent::Code(code) => {
                current.push('`');
                current.push_str(&code);
                current.push('`');
            }
            MarkdownEvent::SoftBreak | MarkdownEvent::HardBreak => {
                if in_table {
                    current.push(' ');
                } else {
                    flush(&mut lines, &mut current, kind);
                    if kind == DocumentLineKind::Quote {
                        current.push_str("│ ");
                    }
                }
            }
            MarkdownEvent::End(Tag::Paragraph) => {
                if !in_table {
                    flush(&mut lines, &mut current, kind);
                    if !matches!(kind, DocumentLineKind::List | DocumentLineKind::Quote) {
                        lines.push(DocumentLine {
                            text: String::new(),
                            kind: DocumentLineKind::Body,
                        });
                    }
                    kind = DocumentLineKind::Body;
                }
            }
            MarkdownEvent::Rule => {
                flush(&mut lines, &mut current, kind);
                lines.push(DocumentLine {
                    text: "────────────────────────────────────────".to_string(),
                    kind: DocumentLineKind::Rule,
                });
            }
            MarkdownEvent::TaskListMarker(checked) => {
                current.push_str(if checked { "☑ " } else { "☐ " });
            }
            MarkdownEvent::Html(html) => {
                let (heading, text, block_break) = markdown_html_text(&html);
                if let Some(level) = heading {
                    flush(&mut lines, &mut current, kind);
                    if !text.is_empty() {
                        lines.push(DocumentLine {
                            text,
                            kind: DocumentLineKind::Heading(level),
                        });
                    }
                    kind = DocumentLineKind::Body;
                } else if !text.is_empty() {
                    current.push_str(&text);
                }
                if block_break {
                    flush(&mut lines, &mut current, kind);
                }
            }
            MarkdownEvent::FootnoteReference(reference) => {
                current.push('[');
                current.push_str(&reference);
                current.push(']');
            }
            _ => {}
        }
    }
    flush(&mut lines, &mut current, kind);
    while lines
        .last()
        .map(|line| line.text.is_empty())
        .unwrap_or(false)
    {
        lines.pop();
    }
    lines
}

fn markdown_html_text(html: &str) -> (Option<u8>, String, bool) {
    let lower = html.to_ascii_lowercase();
    let heading = (1..=6).find(|level| lower.contains(&format!("<h{level}")));
    let block_break = heading.is_some()
        || lower.contains("<br")
        || lower.contains("</p>")
        || lower.contains("</div>");
    let mut visible = String::new();
    let mut inside_tag = false;
    for character in html.chars() {
        match character {
            '<' => inside_tag = true,
            '>' => inside_tag = false,
            _ if !inside_tag => visible.push(character),
            _ => {}
        }
    }
    let visible = visible
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    let visible = visible.split_whitespace().collect::<Vec<_>>().join(" ");
    (heading, visible, block_break)
}

fn previous_char_boundary(text: &str, cursor: usize) -> usize {
    text[..cursor.min(text.len())]
        .char_indices()
        .last()
        .map(|(index, _)| index)
        .unwrap_or(0)
}

fn next_char_boundary(text: &str, cursor: usize) -> usize {
    if cursor >= text.len() {
        return text.len();
    }
    cursor
        + text[cursor..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(0)
}

fn move_cursor_vertical(text: &str, cursor: usize, delta: isize) -> usize {
    let cursor = cursor.min(text.len());
    let line_start = text[..cursor]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    let column = text[line_start..cursor].chars().count();
    if delta < 0 {
        if line_start == 0 {
            return cursor;
        }
        let previous_end = line_start - 1;
        let previous_start = text[..previous_end]
            .rfind('\n')
            .map(|index| index + 1)
            .unwrap_or(0);
        return text[previous_start..previous_end]
            .char_indices()
            .nth(column)
            .map(|(offset, _)| previous_start + offset)
            .unwrap_or(previous_end);
    }
    let line_end = text[cursor..]
        .find('\n')
        .map(|offset| cursor + offset)
        .unwrap_or(text.len());
    if line_end == text.len() {
        return cursor;
    }
    let next_start = line_end + 1;
    let next_end = text[next_start..]
        .find('\n')
        .map(|offset| next_start + offset)
        .unwrap_or(text.len());
    text[next_start..next_end]
        .char_indices()
        .nth(column)
        .map(|(offset, _)| next_start + offset)
        .unwrap_or(next_end)
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
        let service_busy = self.explorer.context_request_in_flight
            || self.explorer.git_status_request_in_flight
            || self.explorer.workspace_status_request_in_flight
            || self.explorer.file_workspace.pending_request.is_some()
            || !self.explorer.pending_directories.is_empty()
            || self
                .explorer
                .codex_prompt_automation
                .tmux_scan_in_flight
                .is_some()
            || !self
                .explorer
                .codex_prompt_automation
                .tmux_scan_queue
                .is_empty();
        let interval = if service_busy {
            SERVICE_POLL_INTERVAL
        } else if !self
            .explorer
            .codex_prompt_automation
            .native_pending
            .is_empty()
        {
            CODEX_PROMPT_DEBOUNCE
        } else {
            SERVICE_IDLE_POLL_INTERVAL
        };
        self.explorer.tick_scheduled = true;
        promise::spawn::spawn(async move {
            smol::Timer::after(interval).await;
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
                    let listing_changed = self.explorer.cache.apply(listing);
                    self.explorer.rows_dirty |= listing_changed;
                    changed |= listing_changed;
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
                        changed |= self.reconcile_file_workspace_context();
                    } else {
                        self.explorer.context_request_in_flight = false;
                    }
                }
                ServiceResponse::TmuxCodexPromptScanCompleted(scan) => {
                    changed |= self.apply_tmux_codex_prompt_scan(scan, Instant::now());
                }
                ServiceResponse::TmuxCodexPromptChoiceCompleted(result) => {
                    changed |= self.apply_tmux_codex_prompt_choice_result(result);
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
                ServiceResponse::WorkspaceStatusLoaded(snapshot) => {
                    self.explorer.workspace_status_request_in_flight = false;
                    let visible_status_changed = self.explorer.workspace_status.codex
                        != snapshot.codex
                        || self.explorer.workspace_status.system.status_label()
                            != snapshot.system.status_label();
                    if self.explorer.workspace_status != snapshot {
                        self.explorer.workspace_status = snapshot;
                    }
                    if visible_status_changed {
                        changed = true;
                    }
                }
                ServiceResponse::FileSearchCompleted(_) => {}
                ServiceResponse::FileLoaded(result) => {
                    if self.explorer.file_workspace.pending_request == Some(result.request_id)
                        && self.explorer.file_workspace.active_path.as_ref() == Some(&result.path)
                    {
                        self.explorer.file_workspace.pending_request = None;
                        self.explorer.file_workspace.error = result.error;
                        if let Some(content) = result.content {
                            log::debug!(
                                "cosmos file workspace: loaded {} bytes from {}",
                                content.len(),
                                result.path.display()
                            );
                            self.explorer.file_workspace.content = content;
                            self.explorer.file_workspace.revision = result.revision;
                            self.explorer.file_workspace.cursor = 0;
                            self.explorer.file_workspace.scroll = 0;
                            self.explorer.file_workspace.horizontal_scroll = 0;
                            self.explorer.file_workspace.dirty = false;
                            self.explorer.file_workspace.rebuild_document();
                        }
                        self.explorer.file_workspace.trace_test_state("file_loaded");
                        changed = true;
                    }
                }
                ServiceResponse::FileSaved(result) => {
                    if self.explorer.file_workspace.pending_request == Some(result.request_id)
                        && self.explorer.file_workspace.active_path.as_ref() == Some(&result.path)
                    {
                        self.explorer.file_workspace.pending_request = None;
                        self.explorer.file_workspace.error = result.error;
                        if self.explorer.file_workspace.error.is_none() {
                            log::debug!("cosmos file workspace: saved {}", result.path.display());
                            self.explorer.file_workspace.revision = result.revision;
                            self.explorer.file_workspace.dirty = false;
                            self.explorer.file_workspace.rebuild_document();
                        }
                        self.explorer.file_workspace.trace_test_state("file_saved");
                        changed = true;
                    }
                }
                ServiceResponse::DirectoryChanged(paths) => {
                    self.explorer.refresh_changed_paths(paths);
                    self.explorer.request_git_status();
                }
                ServiceResponse::WatcherError(error) => {
                    self.explorer.last_error = Some(error);
                    self.explorer.rows_dirty = true;
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
        if now.duration_since(self.explorer.last_git_status_refresh) >= GIT_STATUS_REFRESH_INTERVAL
        {
            self.explorer.last_git_status_refresh = now;
            self.explorer.request_git_status();
        }
        if now.duration_since(self.explorer.last_workspace_status_refresh)
            >= WORKSPACE_STATUS_REFRESH_INTERVAL
        {
            self.explorer.last_workspace_status_refresh = now;
            self.explorer.request_workspace_status();
        }
        changed |= self.codex_prompt_automation_tick(now);
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

    fn audit_codex_prompt(
        &self,
        target_id: String,
        prompt: &CodexPromptMatch,
        result: &'static str,
    ) {
        let event = CodexPromptAuditEvent::new(
            self.explorer.state.codex_prompt_automation_mode,
            prompt.kind,
            target_id,
            prompt.action,
            result,
        );
        self.explorer.service.audit_codex_prompt(
            config::DATA_DIR.join("codex-prompt-automation.jsonl"),
            event,
        );
    }

    fn pane_belongs_to_this_window(&self, pane_id: PaneId) -> bool {
        let mux = Mux::get();
        mux.get_window(self.mux_window_id)
            .map(|window| window.iter().any(|tab| tab.contains_pane(pane_id)))
            .unwrap_or(false)
    }

    pub fn codex_prompt_pane_output(&mut self, pane_id: PaneId) {
        if self.explorer.state.codex_prompt_automation_mode == CodexPromptAutomationMode::Off {
            return;
        }
        self.explorer
            .codex_prompt_automation
            .native_pending
            .insert(pane_id, Instant::now());
        self.schedule_explorer_tick();
    }

    fn queue_tmux_codex_prompt_scan(&mut self, request: TmuxPromptScanRequest) {
        let automation = &mut self.explorer.codex_prompt_automation;
        if automation
            .tmux_scan_in_flight
            .as_ref()
            .map(|current| current.tty_name == request.tty_name)
            .unwrap_or(false)
            || automation
                .tmux_scan_queue
                .iter()
                .any(|queued| queued.tty_name == request.tty_name)
        {
            return;
        }
        automation.tmux_scan_queue.push_back(request);
    }

    pub fn note_codex_prompt_manual_input(&mut self, pane_id: PaneId) {
        self.explorer
            .codex_prompt_automation
            .recent_manual_input
            .insert(pane_id, Instant::now());
    }

    pub fn codex_prompt_automation_key_down(
        &mut self,
        key: &KeyCode,
        modifiers: Modifiers,
    ) -> bool {
        let modifiers = modifiers.remove_positional_mods();
        if modifiers != (Modifiers::SUPER | Modifiers::ALT) {
            return false;
        }
        let next_mode = match key {
            KeyCode::Char('\u{1b}') => Some(CodexPromptAutomationMode::Off),
            KeyCode::Char('p' | 'P') => {
                Some(match self.explorer.state.codex_prompt_automation_mode {
                    CodexPromptAutomationMode::Off => CodexPromptAutomationMode::Observe,
                    CodexPromptAutomationMode::Observe => CodexPromptAutomationMode::Active,
                    CodexPromptAutomationMode::Active => CodexPromptAutomationMode::Off,
                })
            }
            _ => None,
        };
        let Some(next_mode) = next_mode else {
            return false;
        };
        if next_mode == CodexPromptAutomationMode::Off {
            let native_targets = self
                .explorer
                .codex_prompt_automation
                .native_handled
                .keys()
                .map(|pane_id| opaque_native_prompt_target(self.mux_window_id, *pane_id))
                .collect::<Vec<_>>();
            let tmux_targets = self
                .explorer
                .codex_prompt_automation
                .tmux_handled
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            for target in native_targets.into_iter().chain(tmux_targets) {
                clear_codex_prompt_reservation(&target);
            }
            self.explorer.codex_prompt_automation.native_pending.clear();
            self.explorer
                .codex_prompt_automation
                .native_candidates
                .clear();
            self.explorer.codex_prompt_automation.native_handled.clear();
            self.explorer
                .codex_prompt_automation
                .tmux_scan_queue
                .clear();
            self.explorer
                .codex_prompt_automation
                .tmux_candidates
                .clear();
            self.explorer.codex_prompt_automation.tmux_handled.clear();
            self.explorer
                .codex_prompt_automation
                .tmux_choices_pending
                .clear();
        }
        self.explorer.state.codex_prompt_automation_mode = next_mode;
        self.explorer
            .service
            .set_codex_prompt_actions_enabled(next_mode == CodexPromptAutomationMode::Active);
        self.explorer.persist();
        true
    }

    fn native_codex_prompt_surface_is_available(&mut self, pane_id: PaneId) -> bool {
        if self.pane_state(pane_id).overlay.is_some() {
            return false;
        }
        let is_active = self
            .get_active_pane_no_overlay()
            .map(|pane| pane.pane_id() == pane_id)
            .unwrap_or(false);
        !is_active
            || (!self.explorer.focused
                && self.explorer.file_workspace.mode == FileWorkspaceMode::Terminal)
    }

    fn clear_native_codex_prompt(&mut self, pane_id: PaneId) {
        self.explorer
            .codex_prompt_automation
            .native_candidates
            .remove(&pane_id);
        if let Some(previous) = self
            .explorer
            .codex_prompt_automation
            .native_handled
            .remove(&pane_id)
        {
            let target_id = opaque_native_prompt_target(self.mux_window_id, pane_id);
            clear_codex_prompt_reservation(&target_id);
            self.audit_codex_prompt(target_id, &previous, "cleared");
        }
    }

    fn process_native_codex_prompt(&mut self, pane_id: PaneId, now: Instant) -> bool {
        if !self.pane_belongs_to_this_window(pane_id) {
            self.clear_native_codex_prompt(pane_id);
            return false;
        }
        if self
            .explorer
            .codex_prompt_automation
            .recent_manual_input
            .get(&pane_id)
            .map(|input| now.duration_since(*input) < CODEX_PROMPT_MANUAL_INPUT_PAUSE)
            .unwrap_or(false)
        {
            self.explorer
                .codex_prompt_automation
                .native_pending
                .insert(pane_id, now);
            return false;
        }
        if !self.native_codex_prompt_surface_is_available(pane_id) {
            self.explorer
                .codex_prompt_automation
                .native_candidates
                .remove(&pane_id);
            return false;
        }
        let mux = Mux::get();
        let Some(pane) = mux.get_pane(pane_id) else {
            self.clear_native_codex_prompt(pane_id);
            return false;
        };
        let process = pane.get_foreground_process_name(CachePolicy::AllowStale);
        if process_is_tmux(process.as_deref()) {
            if let Some(tty_name) = pane.tty_name() {
                self.queue_tmux_codex_prompt_scan(TmuxPromptScanRequest {
                    outer_pane_id: pane_id,
                    tty_name,
                    tmux_executable: process.unwrap_or_else(|| "tmux".to_string()),
                });
            }
            self.clear_native_codex_prompt(pane_id);
            return false;
        }
        if !process_is_codex(process.as_deref()) {
            self.clear_native_codex_prompt(pane_id);
            return false;
        }
        let dimensions = pane.get_dimensions();
        let bottom = dimensions
            .physical_top
            .saturating_add(dimensions.viewport_rows as isize);
        let (_, lines) = pane.get_lines(dimensions.physical_top..bottom);
        let rows = lines
            .iter()
            .map(|line| line.as_str().into_owned())
            .collect::<Vec<_>>();
        let Some(prompt) = classify_codex_prompt(&rows) else {
            self.clear_native_codex_prompt(pane_id);
            return false;
        };
        if self
            .explorer
            .codex_prompt_automation
            .native_handled
            .contains_key(&pane_id)
        {
            return false;
        }
        let candidate = self
            .explorer
            .codex_prompt_automation
            .native_candidates
            .get(&pane_id);
        let stable = candidate
            .filter(|candidate| candidate.prompt == prompt)
            .map(|candidate| now.duration_since(candidate.first_seen) >= CODEX_PROMPT_DEBOUNCE)
            .unwrap_or(false);
        if !stable {
            let keep_first_seen = candidate
                .filter(|candidate| candidate.prompt == prompt)
                .map(|candidate| candidate.first_seen);
            self.explorer
                .codex_prompt_automation
                .native_candidates
                .insert(
                    pane_id,
                    CodexPromptCandidate {
                        prompt,
                        first_seen: keep_first_seen.unwrap_or(now),
                    },
                );
            self.explorer
                .codex_prompt_automation
                .native_pending
                .insert(pane_id, now);
            return false;
        }
        let target_id = opaque_native_prompt_target(self.mux_window_id, pane_id);
        if !reserve_codex_prompt(&target_id, prompt.fingerprint) {
            self.explorer
                .codex_prompt_automation
                .native_handled
                .insert(pane_id, prompt);
            return false;
        }
        let mode = self.explorer.state.codex_prompt_automation_mode;
        match mode {
            CodexPromptAutomationMode::Off => {}
            CodexPromptAutomationMode::Observe => {
                self.audit_codex_prompt(target_id, &prompt, "observed");
            }
            CodexPromptAutomationMode::Active => {
                let result =
                    pane.key_down(PaneKeyCode::Char(prompt.shortcut), PaneKeyModifiers::NONE);
                if result.is_ok() {
                    self.explorer.codex_prompt_automation.choices_sent += 1;
                    self.audit_codex_prompt(target_id, &prompt, "sent");
                } else {
                    self.audit_codex_prompt(target_id, &prompt, "failed_closed");
                }
            }
        }
        self.explorer
            .codex_prompt_automation
            .native_handled
            .insert(pane_id, prompt);
        true
    }

    fn apply_tmux_codex_prompt_scan(&mut self, scan: TmuxCodexPromptScan, now: Instant) -> bool {
        let Some(request) = self
            .explorer
            .codex_prompt_automation
            .tmux_scan_in_flight
            .take()
        else {
            return false;
        };
        self.explorer.codex_prompt_automation.last_tmux_scan = now;
        if self.explorer.state.codex_prompt_automation_mode == CodexPromptAutomationMode::Off {
            return false;
        }
        if self
            .explorer
            .codex_prompt_automation
            .recent_manual_input
            .get(&request.outer_pane_id)
            .map(|input| now.duration_since(*input) < CODEX_PROMPT_MANUAL_INPUT_PAUSE)
            .unwrap_or(false)
        {
            return false;
        }
        if let Some(error) = scan.error {
            log::debug!("Cosmos Codex prompt tmux scan failed closed: {error}");
            return false;
        }
        let scanned_targets = scan
            .panes
            .iter()
            .map(|pane| pane.target_id.clone())
            .collect::<HashSet<_>>();
        let stale_targets = self
            .explorer
            .codex_prompt_automation
            .tmux_handled
            .keys()
            .filter(|target| !scanned_targets.contains(*target))
            .cloned()
            .collect::<Vec<_>>();
        for target in stale_targets {
            if let Some(previous) = self
                .explorer
                .codex_prompt_automation
                .tmux_handled
                .remove(&target)
            {
                clear_codex_prompt_reservation(&target);
                self.audit_codex_prompt(target.clone(), &previous, "cleared");
            }
            self.explorer
                .codex_prompt_automation
                .tmux_candidates
                .remove(&target);
        }
        self.explorer
            .codex_prompt_automation
            .tmux_candidates
            .retain(|target, _| scanned_targets.contains(target));

        let mut needs_rescan = false;
        let mut changed = false;
        for pane in scan.panes {
            let target_id = pane.target_id.clone();
            let Some(prompt) = pane.prompt.clone() else {
                self.explorer
                    .codex_prompt_automation
                    .tmux_candidates
                    .remove(&target_id);
                if let Some(previous) = self
                    .explorer
                    .codex_prompt_automation
                    .tmux_handled
                    .remove(&target_id)
                {
                    clear_codex_prompt_reservation(&target_id);
                    self.audit_codex_prompt(target_id, &previous, "cleared");
                    changed = true;
                }
                continue;
            };
            if self
                .explorer
                .codex_prompt_automation
                .tmux_handled
                .contains_key(&target_id)
            {
                continue;
            }
            let candidate = self
                .explorer
                .codex_prompt_automation
                .tmux_candidates
                .get(&target_id);
            let stable = candidate
                .filter(|candidate| candidate.prompt == prompt)
                .map(|candidate| now.duration_since(candidate.first_seen) >= CODEX_PROMPT_DEBOUNCE)
                .unwrap_or(false);
            if !stable {
                let keep_first_seen = candidate
                    .filter(|candidate| candidate.prompt == prompt)
                    .map(|candidate| candidate.first_seen);
                self.explorer
                    .codex_prompt_automation
                    .tmux_candidates
                    .insert(
                        target_id,
                        CodexPromptCandidate {
                            prompt,
                            first_seen: keep_first_seen.unwrap_or(now),
                        },
                    );
                needs_rescan = true;
                continue;
            }
            if !reserve_codex_prompt(&target_id, prompt.fingerprint) {
                self.explorer
                    .codex_prompt_automation
                    .tmux_handled
                    .insert(target_id, prompt);
                continue;
            }
            match self.explorer.state.codex_prompt_automation_mode {
                CodexPromptAutomationMode::Off => {}
                CodexPromptAutomationMode::Observe => {
                    self.audit_codex_prompt(target_id.clone(), &prompt, "observed");
                }
                CodexPromptAutomationMode::Active => {
                    self.explorer
                        .codex_prompt_automation
                        .tmux_choices_pending
                        .insert(target_id.clone(), prompt.clone());
                    self.explorer
                        .service
                        .choose_tmux_codex_prompt(pane, prompt.clone());
                }
            }
            self.explorer
                .codex_prompt_automation
                .tmux_handled
                .insert(target_id, prompt);
            changed = true;
        }
        if needs_rescan {
            self.queue_tmux_codex_prompt_scan(request);
        }
        changed
    }

    fn apply_tmux_codex_prompt_choice_result(
        &mut self,
        result: TmuxCodexPromptChoiceResult,
    ) -> bool {
        let Some(prompt) = self
            .explorer
            .codex_prompt_automation
            .tmux_choices_pending
            .remove(&result.target_id)
        else {
            return false;
        };
        if result.error.is_none() && result.fingerprint == prompt.fingerprint {
            self.explorer.codex_prompt_automation.choices_sent += 1;
            self.audit_codex_prompt(result.target_id, &prompt, "sent");
        } else {
            self.audit_codex_prompt(result.target_id, &prompt, "failed_closed");
        }
        true
    }

    fn codex_prompt_automation_tick(&mut self, now: Instant) -> bool {
        if self.explorer.state.codex_prompt_automation_mode == CodexPromptAutomationMode::Off {
            return false;
        }
        self.explorer
            .codex_prompt_automation
            .recent_manual_input
            .retain(|_, input| now.duration_since(*input) < Duration::from_secs(30));
        let due_native = self
            .explorer
            .codex_prompt_automation
            .native_pending
            .iter()
            .filter_map(|(pane_id, output)| {
                (now.duration_since(*output) >= CODEX_PROMPT_DEBOUNCE).then_some(*pane_id)
            })
            .collect::<Vec<_>>();
        let mut changed = false;
        for pane_id in due_native {
            self.explorer
                .codex_prompt_automation
                .native_pending
                .remove(&pane_id);
            changed |= self.process_native_codex_prompt(pane_id, now);
        }

        if self
            .explorer
            .codex_prompt_automation
            .tmux_scan_in_flight
            .is_none()
        {
            if self
                .explorer
                .codex_prompt_automation
                .tmux_scan_queue
                .is_empty()
                && now.duration_since(self.explorer.codex_prompt_automation.last_tmux_scan)
                    >= CODEX_PROMPT_TMUX_SCAN_INTERVAL
            {
                if let Some(pane) = self.get_active_pane_no_overlay() {
                    let process = pane.get_foreground_process_name(CachePolicy::AllowStale);
                    if process_is_tmux(process.as_deref()) {
                        if let Some(tty_name) = pane.tty_name() {
                            self.queue_tmux_codex_prompt_scan(TmuxPromptScanRequest {
                                outer_pane_id: pane.pane_id(),
                                tty_name,
                                tmux_executable: process.unwrap_or_else(|| "tmux".to_string()),
                            });
                        }
                    } else {
                        self.explorer.codex_prompt_automation.last_tmux_scan = now;
                    }
                }
            }
            if let Some(request) = self
                .explorer
                .codex_prompt_automation
                .tmux_scan_queue
                .pop_front()
            {
                self.explorer.service.scan_tmux_codex_prompts(
                    request.tty_name.clone(),
                    request.tmux_executable.clone(),
                );
                self.explorer.codex_prompt_automation.tmux_scan_in_flight = Some(request);
            }
        }
        changed
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
        self.explorer.rows_dirty = true;
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
        self.explorer.file_workspace.explorer_keyboard_mode = false;
        if self.explorer.focused {
            self.explorer.focused = false;
            self.explorer.rows_dirty = true;
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
        self.explorer.rows_dirty = true;
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
            self.explorer.rows_dirty = true;
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
                    self.explorer.rows_dirty = true;
                    if let Some(window) = self.window.as_ref() {
                        window.invalidate();
                    }
                    return;
                }
                let index = self.explorer.state.add_root(path.clone());
                self.explorer.state.follow_mode = FollowMode::Follow;
                self.explorer.selected_path = Some(path);
                self.explorer.selected_root = Some(index);
                self.explorer.rows_dirty = true;
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
                        self.explorer.rows_dirty = true;
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

    pub fn file_workspace_visible(&self) -> bool {
        self.explorer.file_workspace.visible()
    }

    pub fn block_dirty_file_workspace_close(&mut self, assignment: &KeyAssignment) -> bool {
        if !self.explorer.file_workspace.dirty
            || !matches!(
                assignment,
                KeyAssignment::CloseCurrentPane { .. }
                    | KeyAssignment::CloseCurrentTab { .. }
                    | KeyAssignment::QuitApplication
            )
        {
            return false;
        }
        self.explorer.file_workspace.error = Some(
            "Unsaved changes blocked closing. Press Command+Return to save or Command+Shift+D to discard."
                .to_string(),
        );
        self.explorer
            .file_workspace
            .trace_test_state("dirty_close_blocked");
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
        true
    }

    pub fn block_dirty_file_workspace_window_close(&mut self) -> bool {
        if !self.explorer.file_workspace.dirty {
            return false;
        }
        self.explorer.file_workspace.error = Some(
            "Unsaved changes blocked closing. Press Command+Return to save or Command+Shift+D to discard."
                .to_string(),
        );
        self.explorer
            .file_workspace
            .trace_test_state("dirty_close_blocked");
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
        true
    }

    pub fn file_workspace_window_title(&self) -> Option<String> {
        match self.explorer.file_workspace.mode {
            FileWorkspaceMode::Terminal => None,
            FileWorkspaceMode::View | FileWorkspaceMode::Edit => {
                let name = self
                    .explorer
                    .file_workspace
                    .active_path
                    .as_deref()
                    .and_then(Path::file_name)
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| "File Workspace".to_string());
                let mode = if self.explorer.file_workspace.mode == FileWorkspaceMode::Edit {
                    "Edit"
                } else {
                    "Preview"
                };
                Some(format!("{name} — {mode} — Cosmos Term"))
            }
        }
    }

    pub(super) fn active_workspace_root(&self) -> Option<PathBuf> {
        self.explorer
            .display_root
            .as_ref()
            .map(|root| root.path.clone())
    }

    fn file_workspace_owns_context(&self, context: &PaneContext) -> bool {
        self.explorer.file_workspace.owner_pane_id == Some(context.pane_id)
            && self.explorer.file_workspace.owner_tmux_pane_id == context.tmux_pane_id
    }

    fn file_workspace_owns_active_context(&self) -> bool {
        self.explorer
            .active_context
            .as_ref()
            .map(|context| self.file_workspace_owns_context(context))
            .unwrap_or(false)
    }

    fn activate_selected_explorer_row(&mut self) {
        if let Some(index) = self.explorer.selected_index() {
            let file = self
                .explorer
                .rows
                .get(index)
                .filter(|row| row.kind == ExplorerRowKind::File)
                .and_then(|row| row.path.clone());
            if let Some(path) = file {
                self.open_file_workspace_path(path);
            } else {
                self.explorer.toggle_row(index);
            }
        }
    }

    fn toggle_tmux_explorer_keyboard_mode(&mut self) {
        let enabled = !self.explorer.file_workspace.explorer_keyboard_mode;
        self.explorer.file_workspace.explorer_keyboard_mode = enabled;
        if enabled {
            self.focus_explorer();
            if self.explorer.selected_index().is_none() && !self.explorer.rows.is_empty() {
                self.explorer.set_selected_index(0);
            }
            self.explorer
                .file_workspace
                .trace_test_state("explorer_keyboard_entered");
        } else {
            self.blur_explorer();
            self.explorer
                .file_workspace
                .trace_test_state("explorer_keyboard_exited");
        }
    }

    fn reconcile_file_workspace_context(&mut self) -> bool {
        if !self.explorer.file_workspace.visible() {
            return false;
        }
        let context = match self.explorer.active_context.clone() {
            Some(context) => context,
            None => return false,
        };
        let mut changed = false;
        if self.explorer.file_workspace.owner_pane_id == Some(context.pane_id) {
            if let Some(owner_tmux_pane_id) =
                self.explorer.file_workspace.owner_tmux_pane_id.as_deref()
            {
                let owner_geometry = context
                    .tmux_panes
                    .iter()
                    .find(|pane| pane.pane_id == owner_tmux_pane_id)
                    .map(|pane| pane.geometry);
                if owner_geometry.is_none() && !context.tmux_panes.is_empty() {
                    self.return_to_terminal_workspace();
                    return true;
                }
                if self.explorer.file_workspace.owner_tmux_geometry != owner_geometry {
                    self.explorer.file_workspace.owner_tmux_geometry = owner_geometry;
                    changed = true;
                }
            }
        }
        // Pane focus is independent from the file surface and from the global
        // Explorer keyboard region. Keep the viewer attached to its owner
        // until Command+S or a file selection explicitly moves it.
        if !self.file_workspace_owns_context(&context) {
            return changed;
        }
        let root = match self.active_workspace_root() {
            Some(root) => root,
            None => return changed,
        };
        if self.explorer.file_workspace.owner_root.as_ref() == Some(&root) {
            return changed;
        }
        if self.explorer.file_workspace.dirty {
            self.explorer.file_workspace.error = Some(
                "This file has unsaved changes from another pane. Press Command+Return to save or Command+Shift+D to discard."
                    .to_string(),
            );
        } else {
            self.explorer.file_workspace.reset_for_context(
                context.pane_id,
                context.tmux_pane_id,
                context.tmux_window_id,
                context.tmux_geometry,
                root,
            );
        }
        self.update_title();
        true
    }

    pub fn show_file_workspace(&mut self) {
        let (pane_id, tmux_pane_id, tmux_window_id, tmux_geometry) =
            match self.explorer.active_context.as_ref() {
                Some(context) => (
                    context.pane_id,
                    context.tmux_pane_id.clone(),
                    context.tmux_window_id.clone(),
                    context.tmux_geometry,
                ),
                None => {
                    self.explorer.file_workspace.error =
                        Some("Waiting for the active pane directory…".to_string());
                    self.explorer.file_workspace.mode = FileWorkspaceMode::View;
                    return;
                }
            };
        let root = match self.active_workspace_root() {
            Some(root) => root,
            None => return,
        };
        let context_changed = self.explorer.file_workspace.owner_pane_id != Some(pane_id)
            || self.explorer.file_workspace.owner_tmux_pane_id != tmux_pane_id
            || self.explorer.file_workspace.owner_tmux_window_id != tmux_window_id
            || self.explorer.file_workspace.owner_root.as_ref() != Some(&root);
        if context_changed {
            if self.explorer.file_workspace.dirty {
                self.explorer.file_workspace.error = Some(
                    "This file has unsaved changes from another pane. Press Command+Return to save or Command+Shift+D to discard."
                        .to_string(),
                );
            } else {
                self.explorer.file_workspace.reset_for_context(
                    pane_id,
                    tmux_pane_id,
                    tmux_window_id,
                    tmux_geometry,
                    root,
                );
            }
        } else {
            self.explorer.file_workspace.owner_tmux_geometry = tmux_geometry;
        }
        self.explorer.file_workspace.mode = self.explorer.file_workspace.resume_mode;
        self.explorer
            .file_workspace
            .trace_test_state("file_workspace_shown");
        self.update_title();
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    pub fn open_file_workspace_path(&mut self, path: PathBuf) {
        if self.explorer.file_workspace.dirty
            && self.explorer.file_workspace.active_path.as_ref() != Some(&path)
        {
            self.explorer.file_workspace.mode = FileWorkspaceMode::View;
            self.explorer.file_workspace.error = Some(
                "Unsaved changes are still open. Press Command+Return before opening another file."
                    .to_string(),
            );
            return;
        }
        let root = match self.active_workspace_root() {
            Some(root) => root,
            None => return,
        };
        // Opening a file transfers keyboard ownership from the Explorer to
        // the preview. The user can always return to the tree with prefix+0;
        // leaving the Explorer mode active here would make W/S navigate the
        // hidden tree instead of scrolling the newly opened document.
        self.blur_explorer();
        if let Some(context) = self.explorer.active_context.as_ref() {
            self.explorer.file_workspace.owner_pane_id = Some(context.pane_id);
            self.explorer.file_workspace.owner_tmux_pane_id = context.tmux_pane_id.clone();
            self.explorer.file_workspace.owner_tmux_window_id = context.tmux_window_id.clone();
            self.explorer.file_workspace.owner_tmux_geometry = context.tmux_geometry;
        }
        self.explorer.file_workspace.owner_root = Some(root.clone());
        let request_id = self.explorer.file_workspace.next_request_id();
        log::debug!(
            "cosmos file workspace: loading request {} for {}",
            request_id,
            path.display()
        );
        self.explorer.file_workspace.mode = FileWorkspaceMode::View;
        self.explorer.file_workspace.resume_mode = FileWorkspaceMode::View;
        self.explorer.file_workspace.active_path = Some(path.clone());
        self.explorer.file_workspace.content.clear();
        self.explorer.file_workspace.document_lines.clear();
        self.explorer.file_workspace.wrapped_document_lines.clear();
        self.explorer.file_workspace.wrap_columns = 0;
        self.explorer.file_workspace.cursor = 0;
        self.explorer.file_workspace.scroll = 0;
        self.explorer.file_workspace.horizontal_scroll = 0;
        self.explorer.file_workspace.dirty = false;
        self.explorer.file_workspace.revision = None;
        self.explorer.file_workspace.error = None;
        self.explorer.service.file_request(FileRequest::Load {
            request_id,
            root,
            path,
        });
        self.explorer
            .file_workspace
            .trace_test_state("load_requested");
        self.update_title();
        self.schedule_explorer_tick();
    }

    fn save_file_workspace(&mut self) {
        if !self.explorer.file_workspace.dirty {
            return;
        }
        let root = match self
            .explorer
            .file_workspace
            .owner_root
            .clone()
            .or_else(|| self.active_workspace_root())
        {
            Some(root) => root,
            None => return,
        };
        let path = match self.explorer.file_workspace.active_path.clone() {
            Some(path) => path,
            None => return,
        };
        let content = self.explorer.file_workspace.content.clone();
        let expected_revision = self.explorer.file_workspace.revision;
        let request_id = self.explorer.file_workspace.next_request_id();
        log::debug!(
            "cosmos file workspace: saving request {} for {}",
            request_id,
            path.display()
        );
        self.explorer.file_workspace.error = None;
        self.explorer.service.file_request(FileRequest::Save {
            request_id,
            root,
            path,
            content,
            expected_revision,
        });
        self.explorer
            .file_workspace
            .trace_test_state("save_requested");
        self.schedule_explorer_tick();
    }

    pub fn return_to_terminal_workspace(&mut self) {
        if matches!(
            self.explorer.file_workspace.mode,
            FileWorkspaceMode::View | FileWorkspaceMode::Edit
        ) {
            self.explorer.file_workspace.resume_mode = self.explorer.file_workspace.mode;
        }
        self.explorer.file_workspace.mode = FileWorkspaceMode::Terminal;
        self.explorer.file_workspace.tmux_prefix_pending = None;
        self.explorer.file_workspace.explorer_keyboard_mode = false;
        self.explorer.focused = false;
        log::debug!("cosmos file workspace: returned to terminal");
        self.explorer.file_workspace.error = None;
        self.explorer
            .file_workspace
            .trace_test_state("terminal_restored");
        self.update_title();
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    fn tmux_file_workspace_preview_active(&self) -> bool {
        self.explorer.file_workspace.mode == FileWorkspaceMode::View
            && self
                .explorer
                .active_context
                .as_ref()
                .and_then(|context| context.tmux_pane_id.as_ref())
                .is_some()
    }

    pub fn file_workspace_key_down(&mut self, key: &KeyCode, modifiers: Modifiers) -> bool {
        let modifiers = modifiers.remove_positional_mods();
        if matches!(
            (key, modifiers),
            (KeyCode::Char('s' | 'S'), Modifiers::SUPER)
        ) {
            if self.explorer.file_workspace.visible() && self.file_workspace_owns_active_context() {
                self.return_to_terminal_workspace();
            } else {
                self.show_file_workspace();
            }
            return true;
        }
        if self.explorer.file_workspace.visible() {
            if matches!((key, modifiers), (KeyCode::Char('\r'), Modifiers::SUPER)) {
                if self.file_workspace_owns_active_context() {
                    self.save_file_workspace();
                }
                return true;
            }
            if matches!(
                (key, modifiers),
                (KeyCode::Char('e' | 'E'), Modifiers::SUPER)
            ) {
                if !self.file_workspace_owns_active_context() {
                    return true;
                }
                match self.explorer.file_workspace.mode {
                    FileWorkspaceMode::View => {
                        self.blur_explorer();
                        self.explorer.file_workspace.mode = FileWorkspaceMode::Edit;
                        self.explorer.file_workspace.resume_mode = FileWorkspaceMode::Edit;
                        self.explorer.file_workspace.cursor =
                            self.explorer.file_workspace.content.len();
                    }
                    FileWorkspaceMode::Edit => {
                        self.explorer.file_workspace.mode = FileWorkspaceMode::View;
                        self.explorer.file_workspace.resume_mode = FileWorkspaceMode::View;
                        self.explorer.file_workspace.rebuild_document();
                    }
                    _ => {}
                }
                self.explorer
                    .file_workspace
                    .trace_test_state("mode_changed");
                self.update_title();
                return true;
            }
            if self.explorer.file_workspace.dirty
                && matches!(
                    (key, modifiers),
                    (KeyCode::Char('w' | 'W' | 'q' | 'Q'), Modifiers::SUPER)
                )
            {
                self.explorer.file_workspace.error = Some(
                    "Unsaved changes blocked closing. Press Command+Return to save or Command+Shift+D to discard."
                        .to_string(),
                );
                self.explorer
                    .file_workspace
                    .trace_test_state("dirty_close_blocked");
                return true;
            }
            if matches!(
                (key, modifiers),
                (KeyCode::Char('d' | 'D'), mods) if mods == (Modifiers::SUPER | Modifiers::SHIFT)
            ) {
                if !self.file_workspace_owns_active_context() {
                    return true;
                }
                self.explorer.file_workspace.dirty = false;
                if self.reconcile_file_workspace_context() {
                    return true;
                }
                if let Some(path) = self.explorer.file_workspace.active_path.clone() {
                    self.open_file_workspace_path(path);
                } else {
                    self.return_to_terminal_workspace();
                }
                return true;
            }
        }

        // Preserve application/window/tab shortcuts such as protected
        // Command+W and Command+Q. File-workspace commands above are consumed;
        // all other Command chords continue through the normal input map.
        if modifiers.contains(Modifiers::SUPER) {
            return false;
        }

        // prefix+0 is a global third focus region alongside the numbered tmux
        // panes. It owns only the documented Explorer navigation keys and is
        // available even when the file workspace is hidden or owned by a
        // different inner pane.
        if self.explorer.file_workspace.explorer_keyboard_mode {
            match explorer_keyboard_action(key, modifiers) {
                Some(ExplorerKeyboardAction::Move(delta)) => self.explorer.move_selection(delta),
                Some(ExplorerKeyboardAction::Collapse) => self.explorer.collapse_or_parent(),
                Some(ExplorerKeyboardAction::Expand) => self.explorer.expand_or_child(),
                Some(ExplorerKeyboardAction::Activate) => self.activate_selected_explorer_row(),
                Some(ExplorerKeyboardAction::Exit) => self.blur_explorer(),
                None => return false,
            }
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
            return true;
        }

        if !self.explorer.file_workspace.visible() {
            return false;
        }

        // A viewer attached to another pane is presentation state for that
        // pane, not a window-wide input mode. The newly focused pane receives
        // normal terminal input.
        if !self.file_workspace_owns_active_context() {
            return false;
        }

        match self.explorer.file_workspace.mode {
            FileWorkspaceMode::View => {
                match file_preview_action(key, modifiers) {
                    Some(FilePreviewAction::ScrollVertical(delta)) => {
                        self.explorer.file_workspace.scroll =
                            apply_preview_scroll(self.explorer.file_workspace.scroll, delta);
                    }
                    Some(FilePreviewAction::ScrollHorizontal(delta)) => {
                        self.explorer.file_workspace.horizontal_scroll = apply_preview_scroll(
                            self.explorer.file_workspace.horizontal_scroll,
                            delta,
                        );
                    }
                    Some(FilePreviewAction::Exit) => self.return_to_terminal_workspace(),
                    None => return false,
                }
                self.explorer
                    .file_workspace
                    .trace_test_state("file_preview_scrolled");
                true
            }
            FileWorkspaceMode::Edit => {
                let workspace = &mut self.explorer.file_workspace;
                match (key, modifiers) {
                    (KeyCode::LeftArrow, Modifiers::NONE) => {
                        workspace.cursor =
                            previous_char_boundary(&workspace.content, workspace.cursor);
                    }
                    (KeyCode::RightArrow, Modifiers::NONE) => {
                        workspace.cursor = next_char_boundary(&workspace.content, workspace.cursor);
                    }
                    (KeyCode::UpArrow, Modifiers::NONE) => {
                        workspace.cursor =
                            move_cursor_vertical(&workspace.content, workspace.cursor, -1);
                    }
                    (KeyCode::DownArrow, Modifiers::NONE) => {
                        workspace.cursor =
                            move_cursor_vertical(&workspace.content, workspace.cursor, 1);
                    }
                    (KeyCode::Char('\u{8}'), Modifiers::NONE) => {
                        let previous = previous_char_boundary(&workspace.content, workspace.cursor);
                        if previous < workspace.cursor {
                            workspace.content.drain(previous..workspace.cursor);
                            workspace.cursor = previous;
                            workspace.dirty = true;
                        }
                    }
                    (KeyCode::Char('\u{7f}'), Modifiers::NONE) => {
                        let next = next_char_boundary(&workspace.content, workspace.cursor);
                        if next > workspace.cursor {
                            workspace.content.drain(workspace.cursor..next);
                            workspace.dirty = true;
                        }
                    }
                    (KeyCode::Char('\u{1b}'), Modifiers::NONE) => {
                        workspace.mode = FileWorkspaceMode::View;
                        workspace.resume_mode = FileWorkspaceMode::View;
                        workspace.rebuild_document();
                    }
                    (KeyCode::Char('\r'), Modifiers::NONE) => {
                        workspace.content.insert(workspace.cursor, '\n');
                        workspace.cursor += 1;
                        workspace.dirty = true;
                    }
                    (KeyCode::Char('\t'), Modifiers::NONE) => {
                        workspace.content.insert_str(workspace.cursor, "    ");
                        workspace.cursor += 4;
                        workspace.dirty = true;
                    }
                    (KeyCode::Char(character), mods)
                        if mods == Modifiers::NONE || mods == Modifiers::SHIFT =>
                    {
                        if !character.is_control() {
                            workspace.content.insert(workspace.cursor, *character);
                            workspace.cursor += character.len_utf8();
                            workspace.dirty = true;
                        }
                    }
                    _ => {}
                }
                self.explorer.file_workspace.trace_test_state("edit_input");
                self.update_title();
                true
            }
            FileWorkspaceMode::Terminal => false,
        }
    }

    pub fn file_workspace_tmux_prefix_key_down(
        &mut self,
        key: &KeyCode,
        modifiers: Modifiers,
    ) -> bool {
        let is_tmux_prefix = self
            .explorer
            .active_context
            .as_ref()
            .map(|context| {
                context.tmux_pane_id.is_some()
                    && context
                        .tmux_prefixes
                        .iter()
                        .any(|prefix| tmux_key_matches_event(prefix, key, modifiers))
            })
            .unwrap_or(false);
        if is_tmux_prefix {
            self.explorer.file_workspace.tmux_prefix_pending =
                Some((key.clone(), modifiers.remove_positional_mods()));
            self.explorer
                .file_workspace
                .trace_test_state("tmux_prefix_buffered");
            return true;
        }
        false
    }

    pub fn take_file_workspace_tmux_prefix(&mut self) -> Option<(KeyCode, Modifiers)> {
        self.explorer.file_workspace.tmux_prefix_pending.take()
    }

    pub fn file_workspace_tmux_command_key_down(
        &mut self,
        key: &KeyCode,
        modifiers: Modifiers,
    ) -> bool {
        if is_tmux_explorer_toggle_key(key, modifiers) {
            self.toggle_tmux_explorer_keyboard_mode();
            return true;
        }
        // Any real tmux command selects or operates on a tmux region. Leave
        // the global Explorer region before replaying the buffered prefix, so
        // prefix+1 also works when pane 1 is already the workspace owner.
        if self.explorer.file_workspace.explorer_keyboard_mode {
            self.blur_explorer();
            self.explorer
                .file_workspace
                .trace_test_state("explorer_keyboard_exited_for_tmux_command");
        }
        self.explorer
            .file_workspace
            .trace_test_state("tmux_key_passthrough");
        false
    }

    pub fn explorer_key_down(&mut self, key: &KeyCode, modifiers: Modifiers) -> bool {
        if !self.explorer.focused {
            return false;
        }
        // Clicking the Explorer can leave it visually focused after opening a
        // file. It must not become a second keyboard mode over a tmux preview.
        if self.tmux_file_workspace_preview_active() {
            return false;
        }
        let plain_modifiers = modifiers.remove_positional_mods();
        match (key, plain_modifiers) {
            (KeyCode::UpArrow, Modifiers::NONE) => self.explorer.move_selection(-1),
            (KeyCode::DownArrow, Modifiers::NONE) => self.explorer.move_selection(1),
            (KeyCode::LeftArrow, Modifiers::NONE) => self.explorer.collapse_or_parent(),
            (KeyCode::RightArrow, Modifiers::NONE) => self.explorer.expand_or_child(),
            (KeyCode::Char('\r'), Modifiers::NONE) => self.activate_selected_explorer_row(),
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
            1,
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
            1,
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
        layer_num: usize,
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
        let style = TextStyle {
            font: vec![attributes],
            foreground: None,
        };
        let font = self
            .fonts
            .resolve_built_in_font(&style)
            .or_else(|_| self.fonts.resolve_font(&style))?;
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
                    let mut quad = layers.allocate(layer_num)?;
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
                        let mut quad = layers.allocate(layer_num)?;
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

    fn file_workspace_item_hovered(&self, expected: &FileWorkspaceUiItem) -> bool {
        matches!(
            self.last_ui_item.as_ref().map(|item| &item.item_type),
            Some(UIItemType::FileWorkspace(current)) if current == expected
        )
    }

    fn render_file_line(
        &mut self,
        layers: &mut TripleLayerQuadAllocator,
        text: &str,
        top: f32,
        left: f32,
        width: f32,
        height: f32,
        foreground: RgbColor,
        bold: bool,
        logical_font_size: f64,
        monospace: bool,
    ) -> anyhow::Result<()> {
        self.render_explorer_text(
            layers,
            2,
            text,
            top,
            left,
            width,
            height,
            foreground,
            if monospace {
                "JetBrains Mono"
            } else {
                EXPLORER_UI_FONT_FAMILY
            },
            bold,
            logical_font_size,
        )
    }

    fn file_workspace_bounds(&self) -> (usize, usize, usize, usize) {
        if let (Some(owner_pane_id), Some(owner_tmux_window_id), Some(context)) = (
            self.explorer.file_workspace.owner_pane_id,
            self.explorer.file_workspace.owner_tmux_window_id.as_deref(),
            self.explorer.active_context.as_ref(),
        ) {
            if context.pane_id == owner_pane_id
                && context.tmux_window_id.as_deref() != Some(owner_tmux_window_id)
            {
                return (0, 0, 0, 0);
            }
        }
        let border = self.get_os_border();
        let tab_bar_height = if self.show_tab_bar {
            self.tab_bar_pixel_height().unwrap_or(0.) as usize
        } else {
            0
        };
        let top_bar = border.top.get()
            + if self.config.tab_bar_at_bottom {
                0
            } else {
                tab_bar_height
            };
        let window_bottom = self.dimensions.pixel_height.saturating_sub(
            border.bottom.get()
                + self.status_bar_height()
                + if self.config.tab_bar_at_bottom {
                    tab_bar_height
                } else {
                    0
                },
        );
        let (padding_left, padding_top) = self.padding_left_top();
        let cell_width = self.render_metrics.cell_size.width as usize;
        let cell_height = self.render_metrics.cell_size.height as usize;
        let owner_pane_id = self.explorer.file_workspace.owner_pane_id;
        let owner = self
            .get_panes_to_render()
            .into_iter()
            .find(|pane| Some(pane.pane.pane_id()) == owner_pane_id);
        let owner = match owner {
            Some(owner) => owner,
            None => {
                return (0, 0, 0, 0);
            }
        };
        let pane_left = border.left.get()
            + padding_left.max(0.0) as usize
            + owner.left.saturating_mul(cell_width);
        let pane_top =
            top_bar + padding_top.max(0.0) as usize + owner.top.saturating_mul(cell_height);
        let pane_right = pane_left.saturating_add(owner.pixel_width).min(
            self.dimensions
                .pixel_width
                .saturating_sub(border.right.get()),
        );
        let pane_bottom = pane_top
            .saturating_add(owner.pixel_height)
            .min(window_bottom);

        if let Some(geometry) = self.explorer.file_workspace.owner_tmux_geometry {
            let left = pane_left
                .saturating_add(geometry.left.saturating_mul(cell_width))
                .min(pane_right);
            let top = pane_top
                .saturating_add(geometry.top.saturating_mul(cell_height))
                .min(pane_bottom);
            let right = left
                .saturating_add(geometry.width.saturating_mul(cell_width))
                .min(pane_right);
            let bottom = top
                .saturating_add(geometry.height.saturating_mul(cell_height))
                .min(pane_bottom);
            (left, top, right, bottom)
        } else {
            (pane_left, pane_top, pane_right, pane_bottom)
        }
    }

    pub fn paint_file_workspace(
        &mut self,
        layers: &mut TripleLayerQuadAllocator,
    ) -> anyhow::Result<()> {
        if !self.explorer.file_workspace.visible() {
            return Ok(());
        }
        let (left, top, right, bottom) = self.file_workspace_bounds();
        if right <= left || bottom <= top {
            return Ok(());
        }
        let dpi = self.dimensions.dpi;
        let scale = |value| logical_to_physical(value, dpi);
        let width = right - left;
        let header_height = scale(FILE_HEADER_HEIGHT);
        let padding = scale(FILE_CONTENT_PADDING);
        let mode = self.explorer.file_workspace.mode;

        self.filled_rectangle(
            layers,
            2,
            euclid::rect(left as f32, top as f32, width as f32, (bottom - top) as f32),
            FILE_BG.to_linear_tuple_rgba(),
        )?;
        self.filled_rectangle(
            layers,
            2,
            euclid::rect(left as f32, top as f32, width as f32, header_height as f32),
            FILE_HEADER_BG.to_linear_tuple_rgba(),
        )?;
        self.filled_rectangle(
            layers,
            2,
            euclid::rect(
                left as f32,
                (top + header_height - scale(1)) as f32,
                width as f32,
                scale(1) as f32,
            ),
            FILE_BORDER.to_linear_tuple_rgba(),
        )?;
        self.ui_items.push(UIItem {
            x: left,
            y: top,
            width,
            height: bottom - top,
            item_type: UIItemType::FileWorkspace(FileWorkspaceUiItem::Surface),
        });

        let terminal_x = left + scale(12);
        let terminal_width = scale(88);
        if self.file_workspace_item_hovered(&FileWorkspaceUiItem::TerminalTab) {
            self.filled_rectangle(
                layers,
                2,
                euclid::rect(
                    terminal_x as f32,
                    (top + scale(6)) as f32,
                    terminal_width as f32,
                    (header_height - scale(12)) as f32,
                ),
                HOVER_BG.to_linear_tuple_rgba(),
            )?;
        }
        self.render_file_line(
            layers,
            "‹ TERMINAL",
            top as f32,
            terminal_x as f32,
            terminal_width as f32,
            header_height as f32,
            MUTED,
            false,
            12.0,
            false,
        )?;
        self.ui_items.push(UIItem {
            x: terminal_x,
            y: top,
            width: terminal_width,
            height: header_height,
            item_type: UIItemType::FileWorkspace(FileWorkspaceUiItem::TerminalTab),
        });

        let path_label = self
            .explorer
            .file_workspace
            .active_path
            .as_deref()
            .map(|path| {
                self.active_workspace_root()
                    .and_then(|root| path.strip_prefix(root).ok().map(Path::to_path_buf))
                    .unwrap_or_else(|| path.to_path_buf())
                    .display()
                    .to_string()
            })
            .unwrap_or_else(|| "FILE WORKSPACE".to_string());
        let dirty_suffix = if self.explorer.file_workspace.dirty {
            "  ●"
        } else {
            ""
        };
        let title = format!("{path_label}{dirty_suffix}");
        let title_left = terminal_x + terminal_width + scale(12);
        let action_width = scale(74);
        let action_x = right.saturating_sub(action_width + scale(12));
        self.render_file_line(
            layers,
            &title,
            top as f32,
            title_left as f32,
            action_x.saturating_sub(title_left + scale(8)) as f32,
            header_height as f32,
            TEXT,
            false,
            13.0,
            false,
        )?;

        if self.explorer.file_workspace.active_path.is_some() {
            let hovered = self.file_workspace_item_hovered(&FileWorkspaceUiItem::EditToggle);
            if hovered || mode == FileWorkspaceMode::Edit {
                self.filled_rectangle(
                    layers,
                    2,
                    euclid::rect(
                        action_x as f32,
                        (top + scale(6)) as f32,
                        action_width as f32,
                        (header_height - scale(12)) as f32,
                    ),
                    if mode == FileWorkspaceMode::Edit {
                        ACTIVE_SELECTION_BG
                    } else {
                        HOVER_BG
                    }
                    .to_linear_tuple_rgba(),
                )?;
            }
            self.render_file_line(
                layers,
                if mode == FileWorkspaceMode::Edit {
                    "PREVIEW"
                } else {
                    "EDIT"
                },
                top as f32,
                (action_x + scale(10)) as f32,
                action_width.saturating_sub(scale(20)) as f32,
                header_height as f32,
                TEXT,
                false,
                12.0,
                false,
            )?;
            self.ui_items.push(UIItem {
                x: action_x,
                y: top,
                width: action_width,
                height: header_height,
                item_type: UIItemType::FileWorkspace(FileWorkspaceUiItem::EditToggle),
            });
        }

        let content_top = top + header_height;
        if let Some(error) = self.explorer.file_workspace.error.clone() {
            self.render_file_line(
                layers,
                &format!("Unable to open file: {error}"),
                (content_top + padding) as f32,
                (left + padding) as f32,
                width.saturating_sub(padding * 2) as f32,
                scale(28) as f32,
                ERROR,
                false,
                FILE_BODY_FONT_LOGICAL_SIZE,
                false,
            )?;
            return Ok(());
        }
        if self.explorer.file_workspace.pending_request.is_some()
            && self.explorer.file_workspace.content.is_empty()
        {
            self.render_file_line(
                layers,
                "Opening file…",
                (content_top + padding) as f32,
                (left + padding) as f32,
                width.saturating_sub(padding * 2) as f32,
                scale(28) as f32,
                MUTED,
                false,
                FILE_BODY_FONT_LOGICAL_SIZE,
                false,
            )?;
            return Ok(());
        }
        if self.explorer.file_workspace.active_path.is_none() {
            self.render_file_line(
                layers,
                "Select a file from the Explorer",
                (content_top + scale(44)) as f32,
                (left + padding) as f32,
                width.saturating_sub(padding * 2) as f32,
                scale(30) as f32,
                MUTED,
                false,
                FILE_BODY_FONT_LOGICAL_SIZE,
                false,
            )?;
            return Ok(());
        }

        let body_left = left + padding;
        let body_right = right.saturating_sub(padding);
        let body_top = content_top + scale(14);
        if mode == FileWorkspaceMode::Edit {
            let lines = self
                .explorer
                .file_workspace
                .content
                .split('\n')
                .map(str::to_string)
                .collect::<Vec<_>>();
            let cursor_prefix = &self.explorer.file_workspace.content[..self
                .explorer
                .file_workspace
                .cursor
                .min(self.explorer.file_workspace.content.len())];
            let cursor_line = cursor_prefix.bytes().filter(|byte| *byte == b'\n').count();
            let cursor_column = cursor_prefix
                .rsplit('\n')
                .next()
                .unwrap_or("")
                .chars()
                .count();
            let row_height = scale(22);
            let capacity = bottom.saturating_sub(body_top + scale(8)) / row_height;
            if cursor_line < self.explorer.file_workspace.scroll {
                self.explorer.file_workspace.scroll = cursor_line;
            } else if cursor_line >= self.explorer.file_workspace.scroll + capacity.max(1) {
                self.explorer.file_workspace.scroll =
                    cursor_line.saturating_sub(capacity.saturating_sub(1));
            }
            let scroll = self
                .explorer
                .file_workspace
                .scroll
                .min(lines.len().saturating_sub(capacity.max(1)));
            self.explorer.file_workspace.scroll = scroll;
            let number_width = scale(48);
            for (screen_line, (line_index, line)) in lines
                .into_iter()
                .enumerate()
                .skip(scroll)
                .take(capacity)
                .enumerate()
            {
                let y = body_top + screen_line * row_height;
                self.render_file_line(
                    layers,
                    &(line_index + 1).to_string(),
                    y as f32,
                    body_left as f32,
                    number_width.saturating_sub(scale(10)) as f32,
                    row_height as f32,
                    MUTED,
                    false,
                    12.0,
                    true,
                )?;
                self.render_file_line(
                    layers,
                    &line,
                    y as f32,
                    (body_left + number_width) as f32,
                    body_right.saturating_sub(body_left + number_width) as f32,
                    row_height as f32,
                    TEXT,
                    false,
                    FILE_CODE_FONT_LOGICAL_SIZE,
                    true,
                )?;
                if line_index == cursor_line {
                    let caret_x = body_left + number_width + scale(8) * cursor_column.min(500);
                    self.filled_rectangle(
                        layers,
                        2,
                        euclid::rect(
                            caret_x as f32,
                            (y + scale(3)) as f32,
                            scale(1) as f32,
                            row_height.saturating_sub(scale(6)) as f32,
                        ),
                        TEXT.to_linear_tuple_rgba(),
                    )?;
                }
            }
        } else {
            let approximate_columns = body_right.saturating_sub(body_left) / scale(8).max(1);
            self.explorer
                .file_workspace
                .ensure_wrapped_document(approximate_columns);
            let lines = self.explorer.file_workspace.wrapped_document_lines.clone();
            let row_height = scale(27);
            let capacity = bottom.saturating_sub(body_top + scale(8)) / row_height;
            let scroll = self
                .explorer
                .file_workspace
                .scroll
                .min(lines.len().saturating_sub(capacity.max(1)));
            self.explorer.file_workspace.scroll = scroll;
            let max_horizontal_scroll = lines
                .iter()
                .map(|line| line.text.chars().count())
                .max()
                .unwrap_or(0)
                .saturating_sub(1);
            let horizontal_scroll = self
                .explorer
                .file_workspace
                .horizontal_scroll
                .min(max_horizontal_scroll);
            self.explorer.file_workspace.horizontal_scroll = horizontal_scroll;
            for (screen_line, line) in lines.into_iter().skip(scroll).take(capacity).enumerate() {
                let y = body_top + screen_line * row_height;
                let (foreground, bold, font_size, monospace, indent) = match line.kind {
                    DocumentLineKind::Heading(level) => (
                        RgbColor::new_8bpc(230, 230, 230),
                        true,
                        match level {
                            1 => 22.0,
                            2 => 19.0,
                            _ => 16.0,
                        },
                        false,
                        0,
                    ),
                    DocumentLineKind::Code => (TEXT, false, 14.0, true, scale(12)),
                    DocumentLineKind::Quote => (MUTED, false, 15.0, false, scale(10)),
                    DocumentLineKind::List => (TEXT, false, 15.0, false, scale(8)),
                    DocumentLineKind::Rule => (FILE_BORDER, false, 15.0, false, 0),
                    DocumentLineKind::Body => (TEXT, false, 15.0, false, 0),
                };
                if line.kind == DocumentLineKind::Code {
                    self.filled_rectangle(
                        layers,
                        2,
                        euclid::rect(
                            body_left as f32,
                            y as f32,
                            body_right.saturating_sub(body_left) as f32,
                            row_height as f32,
                        ),
                        FILE_CODE_BG.to_linear_tuple_rgba(),
                    )?;
                }
                let display_text = if horizontal_scroll == 0 {
                    line.text.clone()
                } else {
                    line.text.chars().skip(horizontal_scroll).collect()
                };
                self.render_file_line(
                    layers,
                    &display_text,
                    y as f32,
                    (body_left + indent) as f32,
                    body_right.saturating_sub(body_left + indent) as f32,
                    row_height as f32,
                    if display_text.contains("‹http") {
                        FILE_LINK
                    } else {
                        foreground
                    },
                    bold,
                    font_size,
                    monospace,
                )?;
            }
        }
        Ok(())
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
            if self.explorer.workspace_status.codex.active_loops > 0 {
                STATUS_BAR_LIVE
            } else {
                MUTED
            },
            STATUS_BAR_BG,
            false,
            STATUS_BAR_FONT_LOGICAL_SIZE,
        )?;

        let usage = truncate_to_width(&self.explorer.workspace_status.codex.usage_label(), 42);
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

        let loop_label = match self.explorer.workspace_status.codex.active_loops {
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

        let mut capacity_right = loops_left + loops_width;
        if let Some(label) = self.explorer.workspace_status.system.status_label() {
            let capacity_left = capacity_right + scale(12);
            let logical_width = (UnicodeWidthStr::width(label.as_str()) * 7 + 12).clamp(100, 210);
            let capacity_width = scale(logical_width).min(width.saturating_sub(capacity_left));
            if capacity_width > 0 && capacity_left < width {
                self.render_explorer_line(
                    layers,
                    &label,
                    top as f32,
                    capacity_left as f32,
                    capacity_width as f32,
                    height as f32,
                    STATUS_BAR_TEXT,
                    STATUS_BAR_BG,
                    false,
                    STATUS_BAR_FONT_LOGICAL_SIZE,
                )?;
                capacity_right = capacity_left + capacity_width;
            }
        }

        let prompt_mode = self.explorer.state.codex_prompt_automation_mode;
        let prompt_label = if self.explorer.codex_prompt_automation.choices_sent == 0 {
            format!("Prompts {}", prompt_mode.label())
        } else {
            format!(
                "Prompts {} · {}",
                prompt_mode.label(),
                self.explorer.codex_prompt_automation.choices_sent
            )
        };
        let prompt_left = capacity_right + scale(12);
        let prompt_logical_width =
            (UnicodeWidthStr::width(prompt_label.as_str()) * 7 + 12).clamp(96, 160);
        let prompt_width = scale(prompt_logical_width).min(width.saturating_sub(prompt_left));
        if prompt_width > 0 && prompt_left < width {
            self.render_explorer_line(
                layers,
                &prompt_label,
                top as f32,
                prompt_left as f32,
                prompt_width as f32,
                height as f32,
                if prompt_mode == CodexPromptAutomationMode::Active {
                    STATUS_BAR_LIVE
                } else {
                    MUTED
                },
                STATUS_BAR_BG,
                false,
                STATUS_BAR_FONT_LOGICAL_SIZE,
            )?;
            capacity_right = prompt_left + prompt_width;
        }

        if let Some(reset) = self
            .explorer
            .workspace_status
            .codex
            .reset_label(SystemTime::now())
        {
            let reset_width = scale(120).min(width);
            let reset_left = width.saturating_sub(reset_width + scale(8));
            if reset_left > capacity_right.saturating_add(scale(8)) {
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

    pub fn mouse_event_file_workspace(
        &mut self,
        item: FileWorkspaceUiItem,
        event: MouseEvent,
        context: &dyn WindowOps,
    ) {
        context.set_cursor(Some(MouseCursor::Arrow));
        match event.kind {
            MouseEventKind::Press(MousePress::Left) => match item {
                FileWorkspaceUiItem::TerminalTab => self.return_to_terminal_workspace(),
                FileWorkspaceUiItem::EditToggle => {
                    match self.explorer.file_workspace.mode {
                        FileWorkspaceMode::View => {
                            self.explorer.file_workspace.mode = FileWorkspaceMode::Edit;
                            self.explorer.file_workspace.resume_mode = FileWorkspaceMode::Edit;
                            self.explorer.file_workspace.cursor =
                                self.explorer.file_workspace.content.len();
                        }
                        FileWorkspaceMode::Edit => {
                            self.explorer.file_workspace.mode = FileWorkspaceMode::View;
                            self.explorer.file_workspace.resume_mode = FileWorkspaceMode::View;
                            self.explorer.file_workspace.rebuild_document();
                        }
                        _ => {}
                    }
                    self.update_title();
                    context.invalidate();
                }
                FileWorkspaceUiItem::Surface
                    if self.explorer.file_workspace.mode == FileWorkspaceMode::Edit =>
                {
                    let (left, top, _, _) = self.file_workspace_bounds();
                    let scale = |value| logical_to_physical(value, self.dimensions.dpi);
                    let body_top = top + scale(FILE_HEADER_HEIGHT) + scale(14);
                    let body_left = left + scale(FILE_CONTENT_PADDING) + scale(48);
                    let row_height = scale(22).max(1);
                    let clicked_line = self.explorer.file_workspace.scroll
                        + event.coords.y.saturating_sub(body_top as isize).max(0) as usize
                            / row_height;
                    let clicked_column = event.coords.x.saturating_sub(body_left as isize).max(0)
                        as usize
                        / scale(8).max(1);
                    let mut offset = 0usize;
                    for (index, line) in self
                        .explorer
                        .file_workspace
                        .content
                        .split_inclusive('\n')
                        .enumerate()
                    {
                        if index == clicked_line {
                            let line_without_newline = line.strip_suffix('\n').unwrap_or(line);
                            let column_offset = line_without_newline
                                .char_indices()
                                .nth(clicked_column)
                                .map(|(offset, _)| offset)
                                .unwrap_or(line_without_newline.len());
                            self.explorer.file_workspace.cursor = offset + column_offset;
                            break;
                        }
                        offset += line.len();
                    }
                    context.invalidate();
                }
                FileWorkspaceUiItem::Surface => {}
            },
            MouseEventKind::VertWheel(delta) => {
                if delta > 0 {
                    self.explorer.file_workspace.scroll = self
                        .explorer
                        .file_workspace
                        .scroll
                        .saturating_sub(delta as usize);
                } else {
                    self.explorer.file_workspace.scroll += (-delta) as usize;
                }
                context.invalidate();
            }
            _ => {}
        }
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
                        let selected_file = self
                            .explorer
                            .rows
                            .get(index)
                            .filter(|row| row.kind == ExplorerRowKind::File)
                            .and_then(|row| row.path.clone());
                        let double_click = self
                            .last_mouse_click
                            .as_ref()
                            .map(|click| click.streak >= 2)
                            .unwrap_or(false);
                        if let Some(path) = selected_file {
                            self.open_file_workspace_path(path);
                        } else if double_click {
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

#[cfg(test)]
mod tests {
    use super::{
        apply_preview_scroll, explorer_keyboard_action, file_preview_action,
        is_tmux_explorer_toggle_key, tmux_key_matches_event, ExplorerKeyboardAction,
        FilePreviewAction,
    };
    use ::window::{KeyCode, Modifiers};

    #[test]
    fn tmux_explorer_keyboard_navigation_is_explicit_and_bounded() {
        assert!(is_tmux_explorer_toggle_key(
            &KeyCode::Char('0'),
            Modifiers::NONE
        ));
        assert!(!is_tmux_explorer_toggle_key(
            &KeyCode::Char('1'),
            Modifiers::NONE
        ));
        assert!(tmux_key_matches_event(
            "S-BSpace",
            &KeyCode::Char('\u{7f}'),
            Modifiers::SHIFT
        ));
        assert!(!tmux_key_matches_event(
            "S-BSpace",
            &KeyCode::Char('\u{7f}'),
            Modifiers::NONE
        ));
        assert_eq!(
            explorer_keyboard_action(&KeyCode::Char('w'), Modifiers::NONE),
            Some(ExplorerKeyboardAction::Move(-1))
        );
        assert_eq!(
            explorer_keyboard_action(&KeyCode::Char('s'), Modifiers::NONE),
            Some(ExplorerKeyboardAction::Move(1))
        );
        assert_eq!(
            explorer_keyboard_action(&KeyCode::Char('W'), Modifiers::SHIFT),
            Some(ExplorerKeyboardAction::Move(-5))
        );
        assert_eq!(
            explorer_keyboard_action(&KeyCode::Char('W'), Modifiers::NONE),
            Some(ExplorerKeyboardAction::Move(-5))
        );
        assert_eq!(
            explorer_keyboard_action(&KeyCode::Char('w'), Modifiers::SHIFT),
            Some(ExplorerKeyboardAction::Move(-5))
        );
        assert_eq!(
            explorer_keyboard_action(&KeyCode::Char('S'), Modifiers::SHIFT),
            Some(ExplorerKeyboardAction::Move(5))
        );
        assert_eq!(
            explorer_keyboard_action(&KeyCode::Char('S'), Modifiers::NONE),
            Some(ExplorerKeyboardAction::Move(5))
        );
        assert_eq!(
            explorer_keyboard_action(&KeyCode::Char('s'), Modifiers::SHIFT),
            Some(ExplorerKeyboardAction::Move(5))
        );
        assert_eq!(
            explorer_keyboard_action(&KeyCode::Char('\r'), Modifiers::NONE),
            Some(ExplorerKeyboardAction::Activate)
        );
        assert_eq!(
            explorer_keyboard_action(&KeyCode::Char('x'), Modifiers::NONE),
            None
        );
    }

    #[test]
    fn file_preview_navigation_supports_vertical_and_horizontal_jumps() {
        assert_eq!(
            file_preview_action(&KeyCode::Char('w'), Modifiers::NONE),
            Some(FilePreviewAction::ScrollVertical(-1))
        );
        assert_eq!(
            file_preview_action(&KeyCode::Char('S'), Modifiers::NONE),
            Some(FilePreviewAction::ScrollVertical(5))
        );
        assert_eq!(
            file_preview_action(&KeyCode::Char('a'), Modifiers::NONE),
            Some(FilePreviewAction::ScrollHorizontal(-1))
        );
        assert_eq!(
            file_preview_action(&KeyCode::Char('D'), Modifiers::SHIFT),
            Some(FilePreviewAction::ScrollHorizontal(8))
        );
        assert_eq!(
            file_preview_action(&KeyCode::Char('x'), Modifiers::NONE),
            None
        );
        assert_eq!(apply_preview_scroll(3, -5), 0);
        assert_eq!(apply_preview_scroll(3, 8), 11);
    }
}
