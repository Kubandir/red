use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    terminal,
};
use ratatui::{
    layout::{Constraint, Direction, Layout}, prelude::*, style::Style, widgets::{Block, Borders, Clear, Paragraph}
};
use deunicode::deunicode;
use std::{
    env,
    io::stdout,
    path::{PathBuf, Path},
    fs,
    time::{SystemTime, Instant, Duration},
    collections::{HashSet, HashMap},
    fs::File,
    io::Write,
};
use fuzzy_matcher::skim::SkimMatcherV2;
use thiserror::Error;
use chrono::Local;
#[derive(Debug, Error)]
pub enum EditorError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    #[error("File too large: {0}")]
    FileTooLarge(String),
    #[error("Invalid file: {0}")]
    InvalidFile(String),
    #[error("Cannot edit directory: {0}")]
    IsDirectory(String),
}
#[derive(Debug, PartialEq, Clone)]
enum PopupType {
    None,
    Save,
    Help,
    SaveConfirm(SaveAction),
    OverwriteConfirm(String),
    Find,
    Open,
    InitialMenu,
    ToolMenu,
    RecentFiles,
    JumpToLine,
    Replace,
    FileChanged,
    ReplaceQuery,
    ReplaceWithQuery,
    NewFile,
    NewDirectory,
}
#[derive(Debug, PartialEq)]
enum EditorMode {
    Normal,
    Insert,
    Command,
    Visual,
    Replace,
}
#[derive(Debug, PartialEq, Clone)]
enum SaveAction {
    Exit,
    OpenFile,
}
#[derive(PartialEq, Clone)]
struct RecentFile {
    path: PathBuf,
    exists: bool,
    last_modified: SystemTime,
}
#[derive(Clone)]
struct FileEntry {
    name: String,
    path: PathBuf,
    is_dir: bool,
    is_selected: bool,
    depth: usize,
}
#[derive(Clone)]
struct EditorTab {
    content: Vec<String>,
    cursor_position: (usize, usize),
    filename: Option<PathBuf>,
    modified: bool,
    scroll_offset: u16,
}
#[derive(Clone)]
struct EditorSplit {
    tab_index: usize,
    size: u16,
    is_horizontal: bool,
}
#[derive(Clone)]
struct TextDelta {
    line_index: usize,
    old_line: String,
    new_line: String,
    cursor_before: (usize, usize),
    cursor_after: (usize, usize),
    timestamp: Instant,
}
struct Editor {
    content: Vec<String>,
    cursor_position: (usize, usize),
    filename: Option<PathBuf>,
    undo_stack: Vec<(Vec<String>, (usize, usize))>,
    redo_stack: Vec<(Vec<String>, (usize, usize))>,
    terminal: Terminal<CrosstermBackend<std::io::Stdout>>,
    popup_state: PopupType,
    temp_filename: String,
    status_message: Option<(String, std::time::Instant)>,
    scroll_offset: u16,
    modified: bool,
    search_query: String,
    search_index: Option<usize>,
    highlighted_matches: Vec<(usize, usize)>,
    recent_files: Vec<RecentFile>,
    initial_menu_selection: usize,
    show_initial_menu: bool,
    recent_files_selection: usize,
    has_edited: bool,
    current_dir: PathBuf,
    file_entries: Vec<FileEntry>,
    file_explorer_selection: usize,
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
    current_syntax: Option<String>,
    suggestion_matcher: SkimMatcherV2,
    suggestions: Vec<String>,
    showing_suggestions: bool,
    suggestion_index: usize,
    word_database: HashMap<String, f64>,
    language_keywords: HashSet<String>,
    last_search: String,
    mode: EditorMode,
    show_tree: bool,
    tree_focused: bool,
    show_minimap: bool,
    show_status: bool,
    show_numbers: bool,
    is_fullscreen: bool,
    active_tab: usize,
    tabs: Vec<EditorTab>,
    splits: Vec<EditorSplit>,
    last_file_check: Instant,
    last_modified: Option<SystemTime>,
    last_save_time: Option<SystemTime>,
    tool_menu_selection: usize,
    tools: Vec<(&'static str, &'static str, &'static str)>,
    replace_text: String,
    current_match_index: usize,
    file_tree_scroll_offset: u16,
    last_save_state: Option<Vec<String>>,
    last_edit_time: Instant,
    current_file_path: Option<PathBuf>,
}
use syntect::{
    easy::HighlightLines,
    highlighting::ThemeSet,
    parsing::SyntaxSet,
};
const RED_LOGO: &str = r#"
   ██▀███  ▓█████ ▓████▄
  ▓██ ▒ ██▒▓█   ▀ ▒██▀ ██▌
  ▓██ ░▄█ ▒▓█████ ░██   █▌
  ▒██▀▀█▄  ▒▓█  ▄ ░▓█▄   ▌
  ░██▓ ▒██▒░▒████▒░▒████▓
  ░ ▒▓ ░▓░░░ ▒░ ░ ▒▒▓  ▒
    ░▒ ░ ▒░ ░ ░  ░ ░ ▒  ▒
    ░░   ░    ░    ░ ░  ░
     ░        ░  ░   ░
                   ░      "#;
const ICONS: &[(&str, &str)] = &[
    ("󰆍", "Continue"),
    ("󰈙", "Open File"),
    ("󰋚", "Recent Files"),
    ("󰈔", "New File"),
    ("󰋖", "Help"),
    ("󰗼", "Exit"),
];
const STATUS_ICONS: &[(&str, &str)] = &[
    ("󰆍", "Modified"),
    ("󰆍", "Saved"),
    ("󰆍", "Read Only"),
    ("󰆍", "Error"),
];
const HELP_TEXT: &[(&str, &str, &str)] = &[
    ("Navigation", "", ""),
    ("←↑↓→", "Move cursor", "Basic movement"),
    ("Ctrl+←/→", "Word jump", "Move by words"),
    ("Home/End", "Line edges", "Jump to start/end of line"),
    ("PgUp/PgDn", "Page scroll", "Move by pages"),
    ("File", "", ""),
    ("Ctrl+s", "Save", "Save current file"),
    ("Alt+o", "Open", "Open file"),
    ("Alt+w", "Close", "Close current file"),
    ("Alt+q", "Quit", "Exit editor"),
    ("Layout", "", ""),
    ("Alt+b", "Tree View", "Toggle file explorer sidebar"),
    ("Alt+l", "Line Numbers", "Toggle line number gutter"),
    ("Editing", "", ""),
    ("Ctrl+x", "Cut line", "Cut current line"),
    ("Ctrl+c", "Copy line", "Copy current line"),
    ("Ctrl+v", "Paste line", "Paste from clipboard"),
    ("Ctrl+z", "Undo", "Undo last action"),
    ("Ctrl+y", "Redo", "Redo last action"),
    ("Selection", "", ""),
    ("Alt+a", "Select all", "Select entire file"),
    ("Alt+L", "Select line", "Select current line"),
    ("Alt+W", "Select word", "Select current word"),
    ("Search", "", ""),
    ("Ctrl+f", "Find", "Search in file"),
    ("Ctrl+r", "Replace", "Search and replace"),
    ("Alt+n", "Next match", "Go to next match"),
    ("File Tree", "", ""),
    ("Alt+e", "Switch to explorer", "Switch to file explorer window"),
    ("Alt+n", "New file", "Create new file"),
    ("Alt+d", "New directory", "Create new directory"),
    ("Alt+r", "Rename", "Rename selected item"),
    ("Extra", "", ""),
    ("Alt+t", "Tool Menu", "Open tool menu"),
    ("Alt+p", "Settings", "Open settings"),
    ("Alt+h", "Help", "Show this help")
];
const FOLDER_ICONS: &[(&str, &str)] = &[
    ("node_modules", "󰉋"),
    ("src", "󰉋"),
    ("test", "󰉋"),
    ("docs", "󰉋"),
    ("build", "󰉋"),
    ("dist", "󰉋"),
    (".git", "󰉋"),
    ("", "󰉋"),
];
const FILE_ICONS: &[(&str, &str)] = &[
    ("rs", ""),
    ("go", ""),
    ("py", ""),
    ("js", ""),
    ("jsx", ""),
    ("ts", ""),
    ("tsx", ""),
    ("html", ""),
    ("css", ""),
    ("scss", ""),
    ("cpp", ""),
    ("c", ""),
    ("h", ""),
    ("hpp", ""),
    ("java", ""),
    ("kt", ""),
    ("php", ""),
    ("rb", ""),
    ("cs", "󰌛"),
    ("json", ""),
    ("yaml", ""),
    ("yml", ""),
    ("toml", ""),
    ("xml", "謹"),
    ("ini", ""),
    ("conf", ""),
    ("md", ""),
    ("txt", ""),
    ("pdf", ""),
    ("doc", ""),
    ("docx", ""),
    ("xls", ""),
    ("xlsx", ""),
    ("sh", ""),
    ("bash", ""),
    ("zsh", ""),
    ("fish", ""),
    ("git", ""),
    ("gitignore", ""),
    ("lock", ""),
    ("", ""),
];
struct MultiLineDelta {
    start_line: usize,
    old_lines: Vec<String>,
    new_lines: Vec<String>,
    cursor_before: (usize, usize),
    cursor_after: (usize, usize),
    timestamp: Instant,
    file_id: Option<PathBuf>,
}
use crossterm::execute;
impl Editor {
    fn new() -> std::io::Result<Self> {
        let args: Vec<String> = env::args().collect();
        let filename = args.get(1).map(PathBuf::from);
        let (content, initial_message, show_menu) = if let Some(path) = &filename {
            if path.is_dir() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::IsADirectory,
                    "Cannot edit a directory"
                ));
            }
            if path.exists() {
                let metadata = fs::metadata(path)?;
                if metadata.len() > 100 * 1024 * 1024 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "File is too large (>100MB)"
                    ));
                }
            }
            if path.exists() {
                if let Err(e) = std::fs::OpenOptions::new()
                    .write(true)
                    .open(path)
                {
                    if e.kind() == std::io::ErrorKind::PermissionDenied {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::PermissionDenied,
                            "Permission denied"
                        ));
                    }
                }
            } else {
                if let Some(parent) = path.parent() {
                    if parent.exists() {
                        if let Err(e) = std::fs::OpenOptions::new()
                            .write(true)
                            .create(true)
                            .open(parent.join(".red_test_file"))
                        {
                            if e.kind() == std::io::ErrorKind::PermissionDenied {
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::PermissionDenied,
                                    "Permission denied"
                                ));
                            }
                        } else {
                            let _ = std::fs::remove_file(parent.join(".red_test_file"));
                        }
                    }
                }
            }
            match fs::read_to_string(path) {
                Ok(content) => {
                    let lines: Vec<String> = content.lines().map(String::from).collect();
                    (if lines.is_empty() { vec![String::new()] } else { lines }, None, false)
                },
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    (vec![String::new()], Some(format!("New file: {}", Self::format_path(path))), false)
                },
                Err(e) => return Err(e)
            }
        } else {
            (vec![String::new()], None, true)
        };
        terminal::enable_raw_mode()?;
        let mut stdout = stdout();
        crossterm::execute!(stdout, terminal::EnterAlternateScreen)?;
        let terminal = Terminal::new(CrosstermBackend::new(stdout))?;
        let recent_files = Self::load_recent_files();
        let current_dir = env::current_dir()?;
        let file_entries = Self::read_directory(&current_dir)?;
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let theme_set = ThemeSet::load_defaults();
        let current_syntax = if let Some(path) = &filename {
            Self::detect_syntax(&syntax_set, path)
        } else {
            None
        };
        let mut editor = Self {
            content,
            cursor_position: (0, 0),
            filename,
            terminal,
            popup_state: if show_menu { PopupType::InitialMenu } else { PopupType::None },
            temp_filename: String::new(),
            status_message: if show_menu {
                None
            } else {
                Some((
                    initial_message.unwrap_or_else(|| String::from("Press Alt-H for help")),
                    std::time::Instant::now()
                ))
            },
            scroll_offset: 0,
            modified: false,
            search_query: String::new(),
            search_index: None,
            highlighted_matches: Vec::new(),
            recent_files,
            initial_menu_selection: 0,
            show_initial_menu: show_menu,
            recent_files_selection: 0,
            has_edited: false,
            current_dir,
            file_entries,
            file_explorer_selection: 0,
            syntax_set,
            theme_set,
            current_syntax,
            suggestion_matcher: SkimMatcherV2::default(),
            suggestions: Vec::new(),
            showing_suggestions: false,
            suggestion_index: 0,
            word_database: HashMap::new(),
            language_keywords: HashSet::new(),
            last_search: String::new(),
            mode: EditorMode::Normal,
            show_tree: true,
            tree_focused: false,
            show_minimap: true,
            show_status: true,
            show_numbers: true,
            is_fullscreen: false,
            active_tab: 0,
            tabs: Vec::new(),
            splits: Vec::new(),
            last_file_check: Instant::now(),
            last_modified: None,
            last_save_time: None,
            tool_menu_selection: 0,
            tools: vec![
                ("  ", "Delete Comments", "Remove all comments"),
                ("  ", "Remove Empty lines", "Remove all empty lines"),
                ("  ", "Clear Cache", "Clear editor's cache"),
            ],
            replace_text: String::new(),
            current_match_index: 0,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            file_tree_scroll_offset: 0,
            last_save_state: None,
            last_edit_time: Instant::now(),
            current_file_path: None,
        };
        editor.last_save_state = Some(editor.content.clone());
        if let Some(syntax) = editor.current_syntax.clone() {
            editor.update_word_database_for_syntax(&syntax);
        }
        editor.draw()?;
        Ok(editor)
    }
    fn undo(&mut self) {
        if let Some((previous_state, previous_cursor)) = self.undo_stack.pop() {
            self.redo_stack.push((self.content.clone(), self.cursor_position));
            self.content = previous_state;
            self.cursor_position = previous_cursor;
            self.set_status_message("Undid last action.");
            self.modified = true;
        } else {
            self.set_status_message("No more actions to undo.");
        }
    }
    fn redo(&mut self) {
        if let Some((next_state, next_cursor)) = self.redo_stack.pop() {
            self.undo_stack.push((self.content.clone(), self.cursor_position));
            self.content = next_state;
            self.cursor_position = next_cursor;
            self.set_status_message("Redid last action.");
            self.modified = true;
        } else {
            self.set_status_message("No more actions to redo.");
        }
    }
    fn format_path(path: &Path) -> String {
        let home = env::var("HOME").ok().map(PathBuf::from);
        if let Some(home_path) = home {
            if let Ok(relative) = path.strip_prefix(&home_path) {
                return format!("~/{}", relative.display());
            }
        }
        path.display().to_string()
    }
    fn show_help(&mut self) {
        self.popup_state = PopupType::Help;
    }
    fn draw(&mut self) -> std::io::Result<()> {
        let suggestions_visible = self.showing_suggestions && !self.suggestions.is_empty();
        let current_word = if suggestions_visible {
            self.get_current_word().map(|(word, _)| word)
        } else {
            None
        };
        self.terminal.draw(|frame| {
            let area = frame.size();
            let max_scroll = self.file_entries.len().saturating_sub(1) as u16;
            self.file_tree_scroll_offset = self.file_tree_scroll_offset.min(max_scroll);
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(1),
                    Constraint::Length(1),
                ].as_ref())
                .split(area);
            let main_chunks = if self.show_tree {
                Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Length(30),
                        Constraint::Min(1),
                    ].as_ref())
                    .split(chunks[0])
            } else {
                Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Min(1)])
                    .split(chunks[0])
            };
            if self.show_tree {
                let tree_block = Block::default()
                    .title(if self.tree_focused { "[ Files ]" } else { " Files " })
                    .title_alignment(Alignment::Center)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(if self.tree_focused { Color::Green } else { Color::Cyan }));
                let tree_inner = tree_block.inner(main_chunks[0]);
                frame.render_widget(tree_block, main_chunks[0]);
                let visible_height = tree_inner.height as usize;
                let max_scroll = self.file_entries.len().saturating_sub(visible_height);
                if self.file_explorer_selection >= self.file_tree_scroll_offset as usize + visible_height {
                    self.file_tree_scroll_offset = (self.file_tree_scroll_offset + 1).min(max_scroll as u16);
                } else if self.file_explorer_selection < self.file_tree_scroll_offset as usize {
                    self.file_tree_scroll_offset = self.file_explorer_selection as u16;
                }
                self.file_tree_scroll_offset = self.file_tree_scroll_offset.min(max_scroll as u16);
                let items: Vec<Line> = self.file_entries
                    .iter()
                    .skip(self.file_tree_scroll_offset as usize)
                    .take(visible_height)
                    .enumerate()
                    .map(|(i, entry)| {
                        let actual_index = i + self.file_tree_scroll_offset as usize;
                        let style = if actual_index == self.file_explorer_selection {
                            Style::default()
                                .fg(if self.tree_focused { Color::Green } else { Color::White })
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::White)
                        };
                        let icon = if entry.is_dir {
                            if entry.name == ".." {
                                ""
                            } else {
                                "󰉋"
                            }
                        } else {
                            Self::get_file_icon(&entry.path)
                        };
                        let indent = "  ".repeat(entry.depth);
                        let name = &entry.name;
                        Line::from(vec![
                            Span::raw(indent),
                            Span::styled(
                                format!("{} ", if actual_index == self.file_explorer_selection { "▶" } else { " " }),
                                style
                            ),
                            Span::styled(
                                format!("{} ", icon),
                                if entry.is_dir {
                                    style.fg(Color::Cyan)
                                } else {
                                    style
                                }
                            ),
                            Span::styled(name, style),
                        ])
                    })
                    .collect();
                let text = if items.is_empty() {
                    vec![Line::from(Span::styled("Empty directory", Style::default().fg(Color::Gray)))]
                } else if self.tree_focused {
                    let mut text = items;
                    text.extend([
                        Line::from(""),
                        Line::from(Span::styled("Enter: select, Backspace: up", Style::default().fg(Color::DarkGray)))
                    ]);
                    text
                } else {
                    items
                };
                let paragraph = Paragraph::new(text).alignment(Alignment::Left);
                frame.render_widget(paragraph, tree_inner);
            }
            let editor_area = if self.show_tree { main_chunks[1] } else { main_chunks[0] };
            let title = if let Some(path) = &self.filename {
                format!("─[{}]", Self::format_path(path))
            } else {
                "─[New File]".to_string()
            };
            let block = Block::default()
                .title(title)
                .borders(Borders::ALL);
            let inner = block.inner(editor_area);
            frame.render_widget(block, editor_area);
            if (self.cursor_position.1 as u16) >= inner.height + self.scroll_offset {
                self.scroll_offset = (self.cursor_position.1 as u16).saturating_sub(inner.height) + 1;
            } else if (self.cursor_position.1 as u16) < self.scroll_offset {
                self.scroll_offset = self.cursor_position.1 as u16;
            }
            let text = {
                let terminal_height = inner.height as usize;
                let start_line = self.scroll_offset as usize;
                let end_line = (start_line + terminal_height).min(self.content.len());
                let visible_width = inner.width as usize - if self.show_numbers { 5 } else { 1 };
                if let Some(syntax_name) = &self.current_syntax {
                    if let Some(syntax) = self.syntax_set.find_syntax_by_name(syntax_name) {
                        let mut highlighter = HighlightLines::new(
                            syntax,
                            &self.theme_set.themes["base16-ocean.dark"]
                        );
                        let highlighted: Vec<Line> = self.content[start_line..end_line]
                            .iter()
                            .enumerate()
                            .map(|(idx, line)| {
                                let line_idx = idx + start_line;
                                let mut spans = Vec::new();
                                if self.show_numbers {
                                    spans.push(Span::styled(
                                        format!("{:4} ", line_idx + 1),
                                        Style::default().fg(Color::DarkGray)
                                    ));
                                } else {
                                    spans.push(Span::raw(" "));
                                }
                                let matches: Vec<_> = self.highlighted_matches.iter()
                                    .filter(|&(l, _)| *l == line_idx)
                                    .map(|(_, c)| *c)
                                    .collect();
                                let visible_start = if line_idx == self.cursor_position.1 {
                                    (self.cursor_position.0 / visible_width) * visible_width
                                } else {
                                    0
                                };
                                let visible_text = if line.len() > visible_start {
                                    let end = (visible_start + visible_width).min(line.len());
                                    &line[visible_start..end]
                                } else {
                                    ""
                                };
                                if let Ok(ranges) = highlighter.highlight_line(visible_text, &self.syntax_set) {
                                    let mut last_end = 0;
                                    for (style, text) in ranges {
                                        if text.is_empty() {
                                            continue;
                                        }
                                        let start = last_end;
                                        let end = start + text.len();
                                        let matching_positions: Vec<_> = matches.iter()
                                            .filter(|&&pos| pos >= start + visible_start && pos < end + visible_start)
                                            .map(|&pos| pos - visible_start)
                                            .collect();
                                        if !matching_positions.is_empty() {
                                            for match_pos in matching_positions {
                                                if match_pos > start {
                                                    let prefix = text.get(..match_pos.saturating_sub(start))
                                                        .unwrap_or_default();
                                                    if !prefix.is_empty() {
                                                        spans.push(Span::styled(
                                                            prefix,
                                                            Style::default().fg(Color::Rgb(
                                                                style.foreground.r,
                                                                style.foreground.g,
                                                                style.foreground.b,
                                                            ))
                                                        ));
                                                    }
                                                }
                                                let match_text = text.get(
                                                    match_pos.saturating_sub(start)..
                                                        (match_pos.saturating_sub(start) + self.search_query.len())
                                                            .min(text.len())
                                                ).unwrap_or_default();
                                                if !match_text.is_empty() {
                                                    spans.push(Span::styled(
                                                        match_text,
                                                        Style::default()
                                                            .bg(Color::DarkGray)
                                                            .fg(Color::White)
                                                    ));
                                                }
                                            }
                                        } else {
                                            spans.push(Span::styled(
                                                text,
                                                Style::default().fg(Color::Rgb(
                                                    style.foreground.r,
                                                    style.foreground.g,
                                                    style.foreground.b,
                                                ))
                                            ));
                                        }
                                        last_end = end;
                                    }
                                } else {
                                    spans.push(Span::raw(visible_text));
                                }
                                if line.len() > visible_start + visible_width {
                                }
                                if visible_start > 0 {
                                    spans.insert(if self.show_numbers { 1 } else { 1 },
                                        Span::styled("", Style::default().fg(Color::DarkGray)));
                                }
                                Line::from(spans)
                            })
                            .collect();
                        Text::from(highlighted)
                    } else {
                        Text::from(
                            self.content[start_line..end_line]
                                .iter()
                                .enumerate()
                                .map(|(idx, line)| {
                                    let line_idx = idx + start_line;
                                    let visible_start = if line_idx == self.cursor_position.1 {
                                        (self.cursor_position.0 / visible_width) * visible_width
                                    } else {
                                        0
                                    };
                                    let mut spans = Vec::new();
                                    if self.show_numbers {
                                        spans.push(Span::styled(
                                            format!("{:4} ", line_idx + start_line + 1),
                                            Style::default().fg(Color::DarkGray)
                                        ));
                                    } else {
                                        spans.push(Span::raw(" "));
                                    }
                                    if visible_start > 0 {
                                    }
                                    let visible_text = if line.len() > visible_start {
                                        let end = (visible_start + visible_width).min(line.len());
                                        &line[visible_start..end]
                                    } else {
                                        ""
                                    };
                                    spans.push(Span::raw(visible_text));
                                    if line.len() > visible_start + visible_width {
                                    }
                                    Line::from(spans)
                                })
                                .collect::<Vec<_>>()
                        )
                    }
                } else {
                    Text::from(
                        self.content[start_line..end_line]
                            .iter()
                            .enumerate()
                            .map(|(idx, line)| {
                                let line_idx = idx + start_line;
                                let visible_start = if line_idx == self.cursor_position.1 {
                                    (self.cursor_position.0 / visible_width) * visible_width
                                } else {
                                    0
                                };
                                let mut spans = Vec::new();
                                if self.show_numbers {
                                    spans.push(Span::styled(
                                        format!("{:4} ", line_idx + start_line + 1),
                                        Style::default().fg(Color::DarkGray)
                                    ));
                                } else {
                                    spans.push(Span::raw(" "));
                                }
                                if visible_start > 0 {
                                }
                                let visible_text = if line.len() > visible_start {
                                    let end = (visible_start + visible_width).min(line.len());
                                    &line[visible_start..end]
                                } else {
                                    ""
                                };
                                spans.push(Span::raw(visible_text));
                                if line.len() > visible_start + visible_width {
                                }
                                Line::from(spans)
                            })
                            .collect::<Vec<_>>()
                    )
                }
            };
            let paragraph = Paragraph::new(text);
            frame.render_widget(paragraph, inner);
            if let Some((msg, instant)) = &self.status_message {
                if instant.elapsed() < std::time::Duration::from_secs(2) {
                    let status_area = chunks[1];
                    frame.render_widget(Clear, status_area);
                    let status_icon = if msg.contains("Error") {
                        ""
                    } else if msg.contains("Saved") {
                        ""
                    } else {
                        ""
                    };
                    let status = Paragraph::new(format!(" {} {}", status_icon, msg))
                        .style(Style::default().fg(Color::Gray));
                    frame.render_widget(status, status_area);
                } else {
                    self.status_message = None;
                }
            }
            match &self.popup_state {
                PopupType::Help => {
                    let area = frame.size();
                    let width = area.width.saturating_sub(4).min(100);
                    let height = area.height.saturating_sub(4);
                    let help_area = Rect::new(
                        (area.width.saturating_sub(width)) / 2,
                        (area.height.saturating_sub(height)) / 2,
                        width,
                        height
                    );
                    frame.render_widget(Clear, help_area);
                    let help_block = Block::default()
                        .title(" Keyboard Shortcuts (Use ↑↓ to scroll) ")
                        .title_alignment(Alignment::Center)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Cyan));
                    let inner = help_block.inner(help_area);
                    frame.render_widget(help_block, help_area);
                    let mut text = Vec::new();
                    let max_key_width = 12;
                    let max_action_width = 15;
                    let desc_width = inner.width.saturating_sub(max_key_width as u16 + max_action_width as u16 + 6);
                    for (key, action, desc) in HELP_TEXT {
                        if action.is_empty() {
                            if !text.is_empty() {
                                text.push(Line::from(""));
                            }
                            text.push(Line::from(vec![
                                Span::styled(
                                    format!("─── {} ", key),
                                    Style::default()
                                        .fg(Color::Yellow)
                                        .add_modifier(Modifier::BOLD)
                                ),
                                Span::styled(
                                    "─".repeat((desc_width as usize).saturating_sub(key.len() + 4)),
                                    Style::default().fg(Color::DarkGray)
                                ),
                            ]));
                            continue;
                        }
                        text.push(Line::from(vec![
                            Span::styled(
                                format!("{:width$}", key, width = max_key_width),
                                Style::default().fg(Color::Green)
                            ),
                            Span::raw(" "),
                            Span::styled(
                                format!("{:width$}", action, width = max_action_width),
                                Style::default().fg(Color::White)
                            ),
                            Span::raw(" "),
                            Span::styled(
                                desc.to_string(),
                                Style::default().fg(Color::Gray)
                            )
                        ]));
                    }
                    let help_text = Paragraph::new(text)
                        .alignment(Alignment::Left)
                        .scroll((self.file_tree_scroll_offset, 0));
                    frame.render_widget(help_text, inner);
                }
                PopupType::Save => {
                    let area = Rect::new(
                        area.width / 4,
                        area.height / 2 - 2,
                        area.width / 2,
                        3
                    );
                    frame.render_widget(Clear, area);
                    let popup_block = Block::default()
                        .title("Save As")
                        .title_alignment(Alignment::Center)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::White));
                    let inner_area = popup_block.inner(area);
                    frame.render_widget(popup_block, area);
                    let input = Paragraph::new(format!("Filename: {}", self.temp_filename))
                        .style(Style::default().fg(Color::White));
                    frame.render_widget(input, inner_area);
                    frame.set_cursor(
                        area.x + 11 + self.temp_filename.len() as u16,
                        area.y + 1
                    );
                },
                PopupType::SaveConfirm(action) => {
                    let area = Rect::new(
                        area.width / 4,
                        area.height / 2 - 2,
                        area.width / 2,
                        3
                    );
                    frame.render_widget(Clear, area);
                    let popup_block = Block::default()
                        .title("Unsaved Changes")
                        .title_alignment(Alignment::Center)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Yellow));
                    let inner_area = popup_block.inner(area);
                    frame.render_widget(popup_block, area);
                    let msg = if *action == SaveAction::Exit {
                        "Save before exit? (y/n/c)"
                    } else {
                        "Save unsaved changes? (y/n/c)"
                    };
                    let text = Paragraph::new(msg)
                        .style(Style::default().fg(Color::White))
                        .alignment(Alignment::Center);
                    frame.render_widget(text, inner_area);
                },
                PopupType::OverwriteConfirm(_) => {
                    let area = Rect::new(
                        area.width / 4,
                        area.height / 2 - 2,
                        area.width / 2,
                        3
                    );
                    frame.render_widget(Clear, area);
                    let popup_block = Block::default()
                        .title("File Exists")
                        .title_alignment(Alignment::Center)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Yellow));
                    let inner_area = popup_block.inner(area);
                    frame.render_widget(popup_block, area);
                    let text = Paragraph::new("File exists. Overwrite? (y/n)")
                        .style(Style::default().fg(Color::White))
                        .alignment(Alignment::Center);
                    frame.render_widget(text, inner_area);
                },
                PopupType::Find => {
                    let area = Rect::new(
                        area.width / 4,
                        area.height / 2 - 2,
                        area.width / 2,
                        3
                    );
                    frame.render_widget(Clear, area);
                    let popup_block = Block::default()
                        .title("Find")
                        .title_alignment(Alignment::Center)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::White));
                    let inner_area = popup_block.inner(area);
                    frame.render_widget(popup_block, area);
                    let input = Paragraph::new(format!("Search: {}", self.search_query))
                        .style(Style::default().fg(Color::White));
                    frame.render_widget(input, inner_area);
                    frame.set_cursor(
                        area.x + 9 + self.search_query.len() as u16,
                        area.y + 1
                    );
                },
                PopupType::Open => {
                    let area = Rect::new(
                        area.width / 4,
                        area.height / 2 - 2,
                        area.width / 2,
                        3
                    );
                    frame.render_widget(Clear, area);
                    let popup_block = Block::default()
                        .title("Open File")
                        .title_alignment(Alignment::Center)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::White));
                    let inner_area = popup_block.inner(area);
                    frame.render_widget(popup_block, area);
                    let input = Paragraph::new(format!("Path: {}", self.temp_filename))
                        .style(Style::default().fg(Color::White));
                    frame.render_widget(input, inner_area);
                    frame.set_cursor(
                        area.x + 7 + self.temp_filename.len() as u16,
                        area.y + 1
                    );
                },
                PopupType::InitialMenu => {
                    let menu_block = Block::default()
                        .title(" Red Editor ")
                        .title_alignment(Alignment::Center)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Red));
                    let logo_lines: Vec<&str> = RED_LOGO.lines().collect();
                    let logo_width = logo_lines.iter().map(|l| l.len()).max().unwrap_or(0);
                    let menu_items = if self.has_edited {
                        ICONS
                    } else {
                        &ICONS[1..]
                    };
                    let max_menu_item_width = menu_items.iter()
                        .map(|(icon, text)| format!("  {} {}", icon, text).len())
                        .max()
                        .unwrap_or(0) + 8;
                    let content_width = (logo_width.max(max_menu_item_width) + 4) as u16;
                    let content_height = (logo_lines.len() + menu_items.len() + 4) as u16;
                    let menu_area = Rect::new(
                        (area.width - content_width) / 2,
                        (area.height - content_height) / 2,
                        content_width,
                        content_height
                    );
                    frame.render_widget(Clear, menu_area);
                    frame.render_widget(menu_block.clone(), menu_area);
                    let inner_area = menu_block.inner(menu_area);
                    let logo_area = Rect::new(
                        inner_area.x + (inner_area.width.saturating_sub(logo_width as u16)) / 2,
                        inner_area.y + 1,
                        logo_width as u16,
                        logo_lines.len() as u16
                    );
                    let logo = Paragraph::new(RED_LOGO)
                        .style(Style::default().fg(Color::Red))
                        .alignment(Alignment::Center);
                    frame.render_widget(logo, logo_area);
                    let menu_start = logo_area.y + logo_lines.len() as u16 + 1;
                    for (i, (icon, text)) in menu_items.iter().enumerate() {
                        let style = if i == self.initial_menu_selection {
                            Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::White)
                        };
                        let menu_text = format!("{} {}", icon, text);
                        let menu_item = if i == self.initial_menu_selection {
                            format!("▶ {} ◀", menu_text)
                        } else {
                            format!("  {}  ", menu_text)
                        };
                        let menu_paragraph = Paragraph::new(menu_item)
                            .style(style)
                            .alignment(Alignment::Center);
                        frame.render_widget(
                            menu_paragraph,
                            Rect::new(
                                inner_area.x,
                                menu_start + i as u16,
                                inner_area.width,
                                1
                            )
                        );
                    }
                },
                PopupType::ToolMenu => {
                    let tools = vec![
                        ("󰄾", "Delete Comments", "Remove all comments from the file"),
                        ("󰄾", "Remove Empty Lines", "Remove all empty lines from the file"),
                        ("󰄾", "Clear Cache", "Clear editor's cache"),
                    ];
                    let max_tool_width = tools.iter()
                        .map(|(_, name, desc)| name.len() + desc.len() + 4)
                        .max()
                        .unwrap_or(0) as u16;
                    let tool_height = tools.len() as u16 + 2;
                    let area = Rect::new(
                        (area.width.saturating_sub(max_tool_width)) / 2,
                        (area.height.saturating_sub(tool_height)) / 2,
                        max_tool_width,
                        tool_height,
                    );
                    frame.render_widget(Clear, area);
                    let popup_block = Block::default()
                        .title(" Tools ")
                        .title_alignment(Alignment::Center)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Cyan));
                    let inner_area = popup_block.inner(area);
                    frame.render_widget(popup_block, area);
                    let text: Vec<Line> = tools.iter().enumerate().map(|(i, (icon, name, desc))| {
                        let style = if i == self.tool_menu_selection {
                            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::White)
                        };
                        Line::from(vec![
                            Span::styled(
                                if i == self.tool_menu_selection { format!(" {} ", icon) } else { "   ".to_string() },
                                style
                            ),
                            Span::styled(*name, style),
                            Span::raw(" - "),
                            Span::styled(*desc, Style::default().fg(Color::Gray))
                        ])
                    }).collect();
                    let paragraph = Paragraph::new(text)
                        .alignment(Alignment::Left);
                    frame.render_widget(paragraph, inner_area);
                },
                PopupType::RecentFiles => {
                    let area = Rect::new(
                        area.width / 2 - (area.width / 4) / 2,
                        area.height / 3,
                        area.width / 4,
                        area.height / 3,
                    );
                    frame.render_widget(Clear, area);
                    let popup_block = Block::default()
                        .title(" Recent Files ")
                        .title_alignment(Alignment::Center)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Cyan));
                    let inner_area = popup_block.inner(area);
                    frame.render_widget(popup_block, area);
                    let recent_files_text: Vec<Line> = self.recent_files
                        .iter()
                        .enumerate()
                        .map(|(i, rf)| {
                            let status = if rf.exists { "  " } else { "  " };
                            let path = Self::format_path(&rf.path);
                            let style = if i == self.recent_files_selection {
                                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                            } else {
                                Style::default().fg(Color::White)
                            };
                            Line::from(vec![
                                Span::styled(format!(" {} ", if i == self.recent_files_selection { "󰄾" } else { " " }), style),
                                Span::styled(status, if rf.exists { style } else { style.fg(Color::Red) }),
                                Span::styled(path, style),
                            ])
                        })
                        .collect();
                    let text = if recent_files_text.is_empty() {
                        vec![Line::from(Span::styled("No recent files", Style::default().fg(Color::Gray)))]
                    } else {
                        let mut text = recent_files_text;
                        text.push(Line::from(""));
                        text.push(Line::from(Span::styled("         Enter to select, Esc to cancel", Style::default().fg(Color::DarkGray))));
                        text
                    };
                    let paragraph = Paragraph::new(text)
                        .alignment(Alignment::Left);
                    frame.render_widget(paragraph, inner_area);
                },
                PopupType::None => {
                    let visible_width = inner.width.saturating_sub(if self.show_numbers { 5 } else { 1 }) as usize;
                    let cursor_x = self.cursor_position.0 % visible_width;
                    let base_offset = if self.show_numbers { 5 } else { 1 };
                    let wrap_offset = if cursor_x == 0 && self.cursor_position.0 > 0 {
                        visible_width
                    } else {
                        0
                    };
                    frame.set_cursor(
                        inner.x + cursor_x as u16 + base_offset + (wrap_offset % visible_width) as u16,
                        inner.y + self.cursor_position.1 as u16 - self.scroll_offset
                    );
                },
                PopupType::JumpToLine => {
                    let area = Rect::new(
                        area.width / 4,
                        area.height / 2 - 2,
                        area.width / 2,
                        3
                    );
                    frame.render_widget(Clear, area);
                    let popup_block = Block::default()
                        .title("Jump to Line")
                        .title_alignment(Alignment::Center)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::White));
                    let inner_area = popup_block.inner(area);
                    frame.render_widget(popup_block, area);
                    let input = Paragraph::new(format!("Line number: {}", self.search_query))
                        .style(Style::default().fg(Color::White));
                    frame.render_widget(input, inner_area);
                    frame.set_cursor(
                        area.x + 11 + self.search_query.len() as u16,
                        area.y + 1
                    );
                },
                PopupType::ReplaceQuery => {
                    let area = Rect::new(
                        area.width / 4,
                        area.height / 2 - 2,
                        area.width / 2,
                        5
                    );
                    frame.render_widget(Clear, area);
                    let popup_block = Block::default()
                        .title("Find")
                        .title_alignment(Alignment::Center)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::White));
                    let inner_area = popup_block.inner(area);
                    frame.render_widget(popup_block, area);
                    let input = Paragraph::new(vec![
                        Line::from(format!("Find: {}", self.search_query)),
                        Line::from(""),
                        Line::from(Span::styled("Enter: confirm  Esc: cancel", Style::default().fg(Color::DarkGray)))
                    ])
                        .style(Style::default().fg(Color::White));
                    frame.render_widget(input, inner_area);
                    frame.set_cursor(
                        area.x + 7 + self.search_query.len() as u16,
                        area.y + 1
                    );
                },
                PopupType::ReplaceWithQuery => {
                    let area = Rect::new(
                        area.width / 4,
                        area.height / 2 - 2,
                        area.width / 2,
                        5
                    );
                    frame.render_widget(Clear, area);
                    let popup_block = Block::default()
                        .title("Replace")
                        .title_alignment(Alignment::Center)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::White));
                    let inner_area = popup_block.inner(area);
                    frame.render_widget(popup_block, area);
                    let input = Paragraph::new(vec![
                        Line::from(format!("Replace with: {}", self.replace_text)),
                        Line::from(""),
                        Line::from(Span::styled("Enter: confirm  Esc: cancel", Style::default().fg(Color::DarkGray)))
                    ])
                        .style(Style::default().fg(Color::White));
                    frame.render_widget(input, inner_area);
                    frame.set_cursor(
                        area.x + 14 + self.replace_text.len() as u16,
                        area.y + 1
                    );
                },
                PopupType::FileChanged => {
                    let area = Rect::new(
                        area.width / 4,
                        area.height / 2 - 2,
                        area.width / 2,
                        3
                    );
                    frame.render_widget(Clear, area);
                    let popup_block = Block::default()
                        .title("File Changed")
                        .title_alignment(Alignment::Center)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Yellow));
                    let inner_area = popup_block.inner(area);
                    frame.render_widget(popup_block, area);
                    let text = Paragraph::new("The file has been modified. Reload? (y/n)")
                        .style(Style::default().fg(Color::White))
                        .alignment(Alignment::Center);
                    frame.render_widget(text, inner_area);
                },
                PopupType::Replace => {
                    self.popup_state = PopupType::ReplaceQuery;
                    self.search_query.clear();
                    self.replace_text.clear();
                },
                PopupType::NewFile => {
                    let area = Rect::new(
                        area.width / 4,
                        area.height / 2 - 2,
                        area.width / 2,
                        3
                    );
                    frame.render_widget(Clear, area);
                    let popup_block = Block::default()
                        .title("New File")
                        .title_alignment(Alignment::Center)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::White));
                    let inner_area = popup_block.inner(area);
                    frame.render_widget(popup_block, area);
                    let input = Paragraph::new(format!("Filename: {}", self.temp_filename))
                        .style(Style::default().fg(Color::White));
                    frame.render_widget(input, inner_area);
                    frame.set_cursor(
                        area.x + 11 + self.temp_filename.len() as u16,
                        area.y + 1
                    );
                },
                PopupType::NewDirectory => {
                    let area = Rect::new(
                        area.width / 4,
                        area.height / 2 - 2,
                        area.width / 2,
                        3
                    );
                    frame.render_widget(Clear, area);
                    let popup_block = Block::default()
                        .title("New Directory")
                        .title_alignment(Alignment::Center)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::White));
                    let inner_area = popup_block.inner(area);
                    frame.render_widget(popup_block, area);
                    let input = Paragraph::new(format!("Directory name: {}", self.temp_filename))
                        .style(Style::default().fg(Color::White));
                    frame.render_widget(input, inner_area);
                    frame.set_cursor(
                        area.x + 17 + self.temp_filename.len() as u16,
                        area.y + 1
                    );
                },
                PopupType::FileChanged => {
                    let area = Rect::new(
                        area.width / 4,
                        area.height / 2 - 2,
                        area.width / 2,
                        3
                    );
                    frame.render_widget(Clear, area);
                    let popup_block = Block::default()
                        .title("File Changed")
                        .title_alignment(Alignment::Center)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Yellow));
                    let inner_area = popup_block.inner(area);
                    frame.render_widget(popup_block, area);
                    let text = Paragraph::new("File changed on disk. Reload? (y/n)")
                        .style(Style::default().fg(Color::White))
                        .alignment(Alignment::Center);
                    frame.render_widget(text, inner_area);
                }
            }
            if suggestions_visible {
                if let Some(word) = &current_word {
                    let suggestions_height = (self.suggestions.len() + 2) as u16;
                    let suggestions_width = self.suggestions.iter()
                        .map(|s| s.len())
                        .max()
                        .unwrap_or(0)
                        .max(word.len()) as u16 + 4;
                    let visible_width = inner.width.saturating_sub(if self.show_numbers { 5 } else { 1 }) as usize;
                    let cursor_x = inner.x + (self.cursor_position.0 % visible_width) as u16 + if self.show_numbers { 5 } else { 1 };
                    let cursor_y = inner.y + self.cursor_position.1 as u16 - self.scroll_offset;
                    let mut suggestions_x = cursor_x.saturating_sub(word.len() as u16);
                    if suggestions_x + suggestions_width > inner.x + inner.width {
                        suggestions_x = (inner.x + inner.width).saturating_sub(suggestions_width);
                    }
                    let suggestions_y = if cursor_y + 1 + suggestions_height > inner.height {
                        cursor_y.saturating_sub(suggestions_height)
                    } else {
                        cursor_y + 1
                    };
                    let suggestions_area = Rect::new(
                        suggestions_x,
                        suggestions_y,
                        suggestions_width,
                        suggestions_height.min(inner.height - suggestions_y)
                    );
                    let suggestions_block = Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Cyan))
                        .title(" Suggestions ")
                        .title_alignment(Alignment::Center);
                    frame.render_widget(Clear, suggestions_area);
                    frame.render_widget(suggestions_block.clone(), suggestions_area);
                    let inner_area = suggestions_block.inner(suggestions_area);
                    let suggestion_text: Vec<Line> = self.suggestions.iter().enumerate()
                        .map(|(i, suggestion)| {
                            let style = if i == self.suggestion_index {
                                Style::default()
                                    .bg(Color::Rgb(68, 71, 90))
                                    .fg(Color::Rgb(248, 248, 242))
                                    .add_modifier(Modifier::BOLD)
                            } else {
                                Style::default().fg(Color::Rgb(248, 248, 242))
                            };
                            Line::from(Span::styled(suggestion, style))
                        })
                        .collect();
                    let suggestions_paragraph = Paragraph::new(suggestion_text)
                        .block(Block::default());
                    frame.render_widget(suggestions_paragraph, inner_area);
                }
            }
        })?;
        Ok(())
    }
    fn save(&mut self) -> std::io::Result<()> {
        if self.filename.is_none() {
            self.filename = Some(self.current_dir.join(&self.temp_filename));
            self.popup_state = PopupType::Save;
            return Ok(());
        }
        let path = self.filename.as_ref().unwrap().clone();
        if path.exists() && (matches!(self.popup_state, PopupType::Save) || self.filename.is_none()) {
            self.popup_state = PopupType::OverwriteConfirm(path.to_string_lossy().into_owned());
            return Ok(());
        }
        let content = self.content.join("\n") + "\n";
        match fs::write(&path, &content) {
            Ok(_) => {
                self.modified = false;
                self.popup_state = PopupType::None;
                self.add_to_recent_files(path.clone());
                if let Ok(metadata) = fs::metadata(&path) {
                    let modified = metadata.modified().ok();
                    self.last_modified = modified;
                    self.last_save_time = modified;
                }
                self.set_status_message(format!("Saved {}", Self::format_path(&path)));
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                self.set_status_message("Permission denied. Use 'sudo red' to edit this file.");
                Ok(())
            }
            Err(e) => {
                self.set_status_message(format!("Error saving file: {}", e));
                Ok(())
            }
        }
    }
    fn set_status_message<T: Into<String>>(&mut self, message: T) {
        self.status_message = Some((message.into(), std::time::Instant::now()));
    }
    fn run(&mut self) -> std::io::Result<()> {
        if self.content.is_empty() {
            self.content.push(String::new());
        }
        let panic_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            let _ = terminal::disable_raw_mode();
            let _ = crossterm::execute!(
                std::io::stdout(),
                terminal::LeaveAlternateScreen
            );
            panic_hook(panic_info);
        }));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.run_editor_loop()
        }));
        match result {
            Ok(run_result) => run_result,
            Err(e) => {
                let error_msg = if let Some(s) = e.downcast_ref::<String>() {
                    s.clone()
                } else if let Some(s) = e.downcast_ref::<&str>() {
                    s.to_string()
                } else {
                    "Unknown error".to_string()
                };
                self.log_error(&error_msg);
                Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    error_msg
                ))
            }
        }
    }
    fn run_editor_loop(&mut self) -> std::io::Result<()> {
        let mut last_draw = Instant::now();
        let draw_timeout = std::time::Duration::from_millis(16);
        loop {
            self.check_file_changes()?;
            if last_draw.elapsed() >= draw_timeout {
                if let Err(e) = self.draw() {
                    self.log_error(&format!("Draw error: {}", e));
                    return Err(e);
                }
                last_draw = Instant::now();
            }
            if event::poll(draw_timeout)? {
                match event::read()? {
                    Event::Key(key) => {
                        if key.kind == KeyEventKind::Press {
                            if let Err(e) = self.handle_keypress(key) {
                                self.log_error(&format!("Keypress error: {}", e));
                                return Err(e);
                            }
                        }
                    }
                    Event::Mouse(mouse_event) => {
                        if let Err(e) = self.handle_mouse_event(mouse_event) {
                            self.log_error(&format!("Mouse event error: {}", e));
                            return Err(e);
                        }
                    }
                    Event::Resize(_, _) => {
                        if let Err(e) = self.draw() {
                            self.log_error(&format!("Resize draw error: {}", e));
                            return Err(e);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    fn handle_mouse_event(&mut self, mouse_event: event::MouseEvent) -> std::io::Result<()> {
        match mouse_event.kind {
            event::MouseEventKind::Down(event::MouseButton::Left) => {
                let (x, y) = (mouse_event.column as usize, mouse_event.row as usize);
                self.update_cursor_position_from_mouse(x, y);
            }
            _ => {}
        }
        Ok(())
    }
    fn update_cursor_position_from_mouse(&mut self, x: usize, y: usize) {
        let x_offset = if self.show_numbers { 5 } else { 0 };
        let adjusted_x = if x > x_offset { x - x_offset } else { 0 };
        let line_index = y + self.scroll_offset as usize;
        if line_index < self.content.len() {
            let line = &self.content[line_index];
            let mut char_index = 0;
            let mut visual_position = 0;
            let target_x = adjusted_x;
            for (idx, ch) in line.chars().enumerate() {
                if visual_position >= target_x {
                    break;
                }
                let width = if ch == '\t' {
                    4 - (visual_position % 4)
                } else {
                    1
                };
                char_index = idx + 1;
                visual_position += width;
            }
            if visual_position < target_x && !line.is_empty() {
                char_index = line.chars().count();
            }
            self.cursor_position = (char_index, line_index);
        }
    }
    fn handle_enter_key(&mut self) {
        let current_line = &self.content[self.cursor_position.1];
        let indent = current_line.chars().take_while(|c| c.is_whitespace()).collect::<String>();
        let remainder = current_line[self.cursor_position.0..].to_string();
        self.content[self.cursor_position.1] = current_line[..self.cursor_position.0].to_string();
        self.content.insert(self.cursor_position.1 + 1, format!("{}{}", indent, remainder));
        self.cursor_position.1 += 1;
        self.cursor_position.0 = indent.len();
        self.modified = true;
    }
    fn handle_left_key(&mut self) {
        if self.cursor_position.0 > 0 {
            self.cursor_position.0 -= 1;
        } else if self.cursor_position.1 > 0 {
            self.cursor_position.1 -= 1;
            self.cursor_position.0 = self.content[self.cursor_position.1].len();
        }
    }
    fn handle_right_key(&mut self) {
        let current_line = &self.content[self.cursor_position.1];
        let current_line_len = current_line.len();
        if self.cursor_position.0 < current_line_len {
            self.cursor_position.0 += 1;
        } else if self.cursor_position.1 < self.content.len() - 1 {
            self.cursor_position.1 += 1;
            self.cursor_position.0 = 0;
        }
    }
    fn handle_keypress(&mut self, key: KeyEvent) -> std::io::Result<()> {
            match key.code {
                _ => {}
            }
        if matches!((key.code, key.modifiers),
            (KeyCode::Tab, KeyModifiers::ALT) |
            (KeyCode::Tab, KeyModifiers::NONE))
            && self.handle_suggestion_keys(key) {
            return Ok(());
        }
        match &mut self.popup_state {
            PopupType::Save => {
                match (key.code, key.modifiers) {
                    (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                        self.popup_state = PopupType::None;
                        self.temp_filename.clear();
                    }
                    (KeyCode::Enter, _) => {
                        if !self.temp_filename.is_empty() {
                            self.filename = Some(PathBuf::from(&self.temp_filename));
                            self.temp_filename.clear();
                            self.popup_state = PopupType::None;
                            self.save()?;
                        }
                    }
                    (KeyCode::Esc, _) => {
                        self.popup_state = PopupType::None;
                        self.temp_filename.clear();
                    }
                    (KeyCode::Left, _) if !self.temp_filename.is_empty() => {
                    }
                    (KeyCode::Right, _) if !self.temp_filename.is_empty() => {
                    }
                    (KeyCode::Char(c), _) => {
                        self.temp_filename.push(c);
                    }
                    (KeyCode::Backspace, _) => {
                        self.temp_filename.pop();
                    }
                    _ => {}
                }
            }
            PopupType::Help => {
                match key.code {
                    KeyCode::Esc => {
                        self.popup_state = if self.show_initial_menu {
                            PopupType::InitialMenu
                        } else {
                            PopupType::None
                        };
                    }
                    KeyCode::Up => {
                        if self.file_explorer_selection > 0 {
                            self.file_explorer_selection -= 1;
                            if self.file_explorer_selection < self.file_tree_scroll_offset as usize {
                                self.file_tree_scroll_offset = self.file_explorer_selection as u16;
                            }
                        }
                    }
                    KeyCode::Down => {
                        let visible_height = self.terminal.size()?.height.saturating_sub(2);
                        if self.file_explorer_selection < self.file_entries.len().saturating_sub(1) {
                            self.file_explorer_selection += 1;
                            if self.file_explorer_selection >= (self.file_tree_scroll_offset + visible_height) as usize {
                                let max_scroll = self.file_entries.len().saturating_sub(visible_height as usize);
                                self.file_tree_scroll_offset = (self.file_tree_scroll_offset + 1).min(max_scroll as u16);
                            }
                        }
                    }
                    KeyCode::PageUp => {
                        self.scroll_offset = self.scroll_offset.saturating_sub(10);
                    }
                    KeyCode::PageDown => {
                        self.scroll_offset += 10;
                    }
                    _ => {}
                }
            }
            PopupType::SaveConfirm(action) => {
                let action_clone = action.clone();
                match key.code {
                    KeyCode::Char('y') | KeyCode::Char('Y') => {
                        if let Err(e) = self.handle_save_confirm(true, action_clone) {
                            eprintln!("Error confirming save: {}", e);
                        }
                    }
                    KeyCode::Char('n') | KeyCode::Char('N') => {
                        if let Err(e) = self.handle_save_confirm(false, action_clone) {
                            eprintln!("Error confirming save: {}", e);
                        }
                    }
                    KeyCode::Char('c') | KeyCode::Char('C') | KeyCode::Esc => {
                        self.popup_state = PopupType::None;
                    }
                    _ => {}
                }
            }
            PopupType::OverwriteConfirm(_) => {
                match key.code {
                    KeyCode::Char('y') | KeyCode::Char('Y') => {
                        let path = self.filename.as_ref().unwrap().clone();
                        let content = self.content.join("\n") + "\n";
                        match fs::write(&path, &content) {
                            Ok(_) => {
                                self.modified = false;
                                self.popup_state = PopupType::None;
                                self.add_to_recent_files(path.clone());
                                self.set_status_message(format!("Saved {}", Self::format_path(&path)));
                            }
                            Err(e) => {
                                self.popup_state = PopupType::None;
                                self.set_status_message(format!("Error saving file: {}", e));
                            }
                        }
                    }
                    KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                        self.popup_state = PopupType::Save;
                        self.temp_filename.clear();
                    }
                    _ => {}
                }
            }
            PopupType::Find => {
                match key.code {
                    KeyCode::Enter => {
                        self.find_next();
                        if !self.highlighted_matches.is_empty() {
                            self.popup_state = PopupType::None;
                        }
                    }
                    KeyCode::Esc => {
                        self.popup_state = if self.show_initial_menu {
                            PopupType::InitialMenu
                        } else {
                            PopupType::None
                        };
                        self.search_query.clear();
                        self.highlighted_matches.clear();
                    }
                    KeyCode::Char(c) => {
                        self.handle_search_input(c);
                    }
                    KeyCode::Backspace => {
                        if !self.search_query.is_empty() {
                            self.search_query.pop();
                            if !self.search_query.is_empty() {
                                self.find_next();
                            } else {
                                self.highlighted_matches.clear();
                            }
                        }
                    }
                    _ => {}
                }
            }
            PopupType::Open => {
                match key.code {
                    KeyCode::Enter => {
                        if !self.temp_filename.is_empty() {
                            let path = PathBuf::from(&self.temp_filename);
                            let temp_filename = self.temp_filename.clone();
                            if self.modified {
                                self.temp_filename = temp_filename;
                                self.popup_state = PopupType::SaveConfirm(SaveAction::OpenFile);
                            } else {
                                self.open_file(&path)?;
                                self.temp_filename.clear();
                                self.popup_state = PopupType::None;
                            }
                        }
                    }
                    KeyCode::Esc => {
                        self.popup_state = if self.show_initial_menu {
                            PopupType::InitialMenu
                        } else {
                            PopupType::None
                        };
                        self.temp_filename.clear();
                    }
                    KeyCode::Char(c) => {
                        self.temp_filename.push(c);
                    }
                    KeyCode::Backspace => {
                        self.temp_filename.pop();
                    }
                    _ => {}
                }
            }
            PopupType::InitialMenu => {
                match key.code {
                    KeyCode::Up => {
                        let menu_items = if self.has_edited {
                            ICONS
                        } else {
                            &ICONS[1..]
                        };
                        let max_items = menu_items.len();
                        self.initial_menu_selection = self.initial_menu_selection
                            .checked_sub(1)
                            .unwrap_or(max_items - 1);
                    }
                    KeyCode::Down => {
                        let menu_items = if self.has_edited {
                            ICONS
                        } else {
                            &ICONS[1..]
                        };
                        let max_items = menu_items.len();
                        self.initial_menu_selection = (self.initial_menu_selection + 1) % max_items;
                    }
                    KeyCode::Enter => {
                        let selection = if self.has_edited {
                            self.initial_menu_selection
                        } else {
                            self.initial_menu_selection + 1
                        };
                        match selection {
                            0 => {
                                self.show_initial_menu = false;
                                self.popup_state = PopupType::None;
                            }
                            1 => {
                                self.popup_state = PopupType::Open;
                                self.temp_filename.clear();
                            }
                            2 => {
                                if !self.recent_files.is_empty() {
                                    self.popup_state = PopupType::RecentFiles;
                                    self.recent_files_selection = 0;
                                } else {
                                    self.set_status_message("No recent files");
                                }
                            }
                            3 => {
                                self.content = vec![String::new()];
                                self.cursor_position = (0, 0);
                                self.filename = None;
                                self.modified = false;
                                self.scroll_offset = 0;
                                self.show_initial_menu = false;
                                self.popup_state = PopupType::None;
                            }
                            4 => {
                                self.show_help();
                            }
                            5 => {
                                if self.modified {
                                    self.popup_state = PopupType::SaveConfirm(SaveAction::Exit);
                                } else {
                                    self.cleanup()?;
                                    std::process::exit(0);
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            PopupType::RecentFiles => {
                match key.code {
                    KeyCode::Up => {
                        self.recent_files_selection = self.recent_files_selection
                            .checked_sub(1)
                            .unwrap_or(self.recent_files.len().saturating_sub(1));
                    }
                    KeyCode::Down => {
                        self.recent_files_selection = (self.recent_files_selection + 1) % self.recent_files.len();
                    }
                    KeyCode::Enter => {
                        if let Some(rf) = self.recent_files.get(self.recent_files_selection) {
                            let path = rf.path.clone();
                            if self.modified {
                                self.temp_filename = path.to_string_lossy().into_owned();
                                self.popup_state = PopupType::SaveConfirm(SaveAction::OpenFile);
                            } else {
                                self.open_file(&path)?;
                                self.popup_state = PopupType::None;
                            }
                        }
                    }
                    KeyCode::Esc => {
                        self.popup_state = PopupType::InitialMenu;
                    }
                    _ => {}
                }
            }
            PopupType::None => {
                if self.tree_focused {
                    let visible_height = self.terminal.size()?.height.saturating_sub(2) as usize;
                    let max_scroll = self.file_entries.len().saturating_sub(visible_height);
                    match (key.code, key.modifiers) {
                        (KeyCode::Left, KeyModifiers::CONTROL) => {
                            if self.cursor_position.0 > 0 {
                                let line = &self.content[self.cursor_position.1];
                                let before_cursor = &line[..self.cursor_position.0];
                                if let Some(pos) = before_cursor.rfind(char::is_whitespace) {
                                    self.cursor_position.0 = pos;
                                } else {
                                    self.cursor_position.0 = 0;
                                }
                            }
                        }
                        (KeyCode::Right, KeyModifiers::CONTROL) => {
                            let line = &self.content[self.cursor_position.1];
                            if self.cursor_position.0 < line.len() {
                                let after_cursor = &line[self.cursor_position.0..];
                                if let Some(pos) = after_cursor.find(char::is_whitespace) {
                                    self.cursor_position.0 += pos;
                                } else {
                                    self.cursor_position.0 = line.len();
                                }
                            }
                        }
                        (KeyCode::Up, modifiers) => {
                            if modifiers.contains(KeyModifiers::CONTROL) {
                                if self.cursor_position.1 > 0 {
                                    self.cursor_position.1 = self.cursor_position.1.saturating_sub(5);
                                    let line_len = self.content[self.cursor_position.1].len();
                                    self.cursor_position.0 = self.cursor_position.0.min(line_len);
                                }
                            } else {
                                if self.file_explorer_selection > 0 {
                                    self.file_explorer_selection -= 1;
                                    if self.file_explorer_selection < self.file_tree_scroll_offset as usize {
                                        self.file_tree_scroll_offset = self.file_explorer_selection as u16;
                                    }
                                }
                            }
                        }
                        (KeyCode::Down, modifiers) => {
                            if modifiers.contains(KeyModifiers::CONTROL) {
                                if self.cursor_position.1 < self.content.len() - 1 {
                                    self.cursor_position.1 = (self.cursor_position.1 + 5).min(self.content.len() - 1);
                                    let line_len = self.content[self.cursor_position.1].len();
                                    self.cursor_position.0 = self.cursor_position.0.min(line_len);
                                }
                            } else {
                                let visible_height = self.terminal.size()?.height.saturating_sub(2);
                                if self.file_explorer_selection < self.file_entries.len().saturating_sub(1) {
                                    self.file_explorer_selection += 1;
                                    if self.file_explorer_selection >= (self.file_tree_scroll_offset + visible_height) as usize {
                                        let max_scroll = self.file_entries.len().saturating_sub(visible_height as usize);
                                        self.file_tree_scroll_offset = (self.file_tree_scroll_offset + 1).min(max_scroll as u16);
                                    }
                                }
                            }
                        }
                        (KeyCode::Enter, _) => {
                            if let Some(entry) = self.file_entries.get(self.file_explorer_selection).cloned() {
                                if entry.is_dir {
                                    self.current_dir = entry.path.clone();
                                    self.file_entries = Self::read_directory(&self.current_dir)?;
                                    self.file_explorer_selection = 0;
                                } else {
                                    if self.modified {
                                        self.temp_filename = entry.path.to_string_lossy().into_owned();
                                        self.popup_state = PopupType::SaveConfirm(SaveAction::OpenFile);
                                    } else {
                                        self.open_file(&entry.path)?;
                                        self.tree_focused = false;
                                    }
                                }
                                self.file_explorer_selection = self.file_explorer_selection;
                            }
                        }
                        (KeyCode::Backspace, _) => {
                            if let Some(parent) = self.current_dir.parent() {
                                self.current_dir = parent.to_path_buf();
                                self.file_entries = Self::read_directory(&self.current_dir)?;
                                self.file_explorer_selection = 0;
                            }
                        }
                        (KeyCode::Esc, _) => {
                            self.tree_focused = false;
                        }
                        (KeyCode::Char('e'), KeyModifiers::ALT) => {
                            self.tree_focused = false;
                        }
                        (KeyCode::Char('n'), KeyModifiers::ALT) => {
                            self.popup_state = PopupType::NewFile;
                            self.temp_filename.clear();
                            return Ok(());
                        }
                        (KeyCode::Char('d'), KeyModifiers::ALT) => {
                            if self.tree_focused {
                                self.popup_state = PopupType::NewDirectory;
                                self.temp_filename.clear();
                            }
                            return Ok(());
                        }
                        _ => {}
                    }
                } else {
                    match (key.code, key.modifiers) {
                        (KeyCode::Tab, KeyModifiers::NONE) => {
                            if !self.showing_suggestions || self.suggestions.is_empty() {
                                let spaces = "    ";
                                if self.cursor_position.1 >= self.content.len() {
                                    self.content.push(String::new());
                                }
                                let line = &mut self.content[self.cursor_position.1];
                                line.insert_str(self.cursor_position.0, spaces);
                                self.cursor_position.0 += 4;
                                self.modified = true;
                            } else {
                                self.apply_suggestion();
                            }
                        }
                        (KeyCode::Left, KeyModifiers::NONE) => {
                            self.handle_left_key();
                        }
                        (KeyCode::Right, KeyModifiers::NONE) => {
                            self.handle_right_key();
                        }
                        (KeyCode::Enter, KeyModifiers::NONE) => {
                            self.handle_enter_key();
                        }
                        (KeyCode::Left, modifiers) => {
                            if modifiers.contains(KeyModifiers::ALT) || modifiers.contains(KeyModifiers::CONTROL) {
                                if self.cursor_position.0 > 0 {
                                    let line = &self.content[self.cursor_position.1];
                                    let before_cursor = &line[..self.cursor_position.0];
                                    if let Some(pos) = before_cursor.rfind(char::is_whitespace) {
                                        self.cursor_position.0 = pos + if modifiers.contains(KeyModifiers::ALT) { 1 } else { 0 };
                                    } else {
                                        self.cursor_position.0 = 0;
                                    }
                                }
                            } else {
                                self.handle_left_key();
                            }
                        }
                        (KeyCode::Right, modifiers) => {
                            if modifiers.contains(KeyModifiers::ALT) || modifiers.contains(KeyModifiers::CONTROL) {
                                let line = &self.content[self.cursor_position.1];
                                if self.cursor_position.0 < line.len() {
                                    let after_cursor = &line[self.cursor_position.0..];
                                    let next_space = after_cursor.find(|c: char| c.is_whitespace());
                                    if let Some(space_pos) = next_space {
                                        let slice_after_space = &after_cursor[space_pos..];
                                        if let Some(word_pos) = slice_after_space.find(|c: char| !c.is_whitespace()) {
                                            self.cursor_position.0 += space_pos + word_pos;
                                        } else {
                                            self.cursor_position.0 = line.len();
                                        }
                                    } else {
                                        self.cursor_position.0 = line.len();
                                    }
                                }
                            } else {
                                self.handle_right_key();
                            }
                        }
                        (KeyCode::Up, modifiers) => {
                            if self.tree_focused {
                                if self.file_explorer_selection > 0 {
                                    self.file_explorer_selection -= 1;
                                    if self.file_explorer_selection < self.file_tree_scroll_offset as usize {
                                        self.file_tree_scroll_offset = self.file_explorer_selection as u16;
                                    }
                                }
                            } else {
                                if modifiers.contains(KeyModifiers::CONTROL) {
                                    if self.cursor_position.1 > 0 {
                                        self.cursor_position.1 = self.cursor_position.1.saturating_sub(5).max(0);
                                        let line_len = self.content[self.cursor_position.1].len();
                                        self.cursor_position.0 = self.cursor_position.0.min(line_len);
                                    }
                                } else {
                                    if self.cursor_position.1 > 0 {
                                        self.cursor_position.1 -= 1;
                                        let line_len = self.content[self.cursor_position.1].len();
                                        self.cursor_position.0 = self.cursor_position.0.min(line_len);
                                    }
                                }
                            }
                        }
                        (KeyCode::Down, modifiers) => {
                            if self.tree_focused {
                                if self.file_explorer_selection < self.file_entries.len() - 1 {
                                    self.file_explorer_selection += 1;
                                    let max_scroll = self.file_entries.len().saturating_sub(1) as u16;
                                    if self.file_explorer_selection >= (self.file_tree_scroll_offset + max_scroll) as usize {
                                        self.file_tree_scroll_offset = (self.file_explorer_selection - max_scroll as usize) as u16;
                                    }
                                }
                            } else {
                                if modifiers.contains(KeyModifiers::CONTROL) {
                                    if self.cursor_position.1 < self.content.len() - 1 {
                                        self.cursor_position.1 = (self.cursor_position.1 + 5).min(self.content.len() - 1);
                                        let line_len = self.content[self.cursor_position.1].len();
                                        self.cursor_position.0 = self.cursor_position.0.min(line_len);
                                    }
                                } else {
                                    if self.cursor_position.1 < self.content.len() - 1 {
                                        self.cursor_position.1 += 1;
                                        let line_len = self.content[self.cursor_position.1].len();
                                        self.cursor_position.0 = self.cursor_position.0.min(line_len);
                                    }
                                }
                            }
                        }
                        (KeyCode::Home, _) => {
                            self.cursor_position.0 = 0;
                        }
                        (KeyCode::End, _) => {
                            self.cursor_position.0 = self.content[self.cursor_position.1].len();
                        }
                        (KeyCode::PageUp, _) => {
                            let page_size = self.terminal.size().unwrap().height as usize;
                            self.cursor_position.1 = self.cursor_position.1.saturating_sub(page_size);
                            let line_len = self.content[self.cursor_position.1].len();
                            self.cursor_position.0 = self.cursor_position.0.min(line_len);
                        }
                        (KeyCode::PageDown, _) => {
                            let page_size = self.terminal.size().unwrap().height as usize;
                            self.cursor_position.1 = (self.cursor_position.1 + page_size).min(self.content.len() - 1);
                            let line_len = self.content[self.cursor_position.1].len();
                            self.cursor_position.0 = self.cursor_position.0.min(line_len);
                        }
                        (KeyCode::Char('s'), KeyModifiers::CONTROL) => {
                            self.save()?;
                        }
                        (KeyCode::Char('o'), KeyModifiers::ALT) => {
                            self.popup_state = PopupType::Open;
                            self.temp_filename.clear();
                        }
                        (KeyCode::Char('w'), KeyModifiers::ALT) => {
                            self.try_close_tab();
                        }
                        (KeyCode::Char('q'), KeyModifiers::ALT) => {
                            self.try_exit();
                        }
                        (KeyCode::Char('b'), KeyModifiers::ALT) => {
                            self.show_tree = !self.show_tree;
                            if (!self.show_tree) {
                                self.tree_focused = false;
                            }
                        }
                        (KeyCode::Char('l'), KeyModifiers::ALT) => {
                            self.show_numbers = !self.show_numbers;
                        }
                        (KeyCode::Char('x'), KeyModifiers::CONTROL) => {
                            if self.cursor_position.1 < self.content.len() {
                                let _line = self.content.remove(self.cursor_position.1);
                                if !_line.is_empty() {
                                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                        if let Err(e) = clipboard.set_text(_line) {
                                            self.set_status_message(&format!("Failed to cut: {}", e));
                                            return Ok(());
                                        }
                                    }
                                }
                                if self.content.is_empty() {
                                    self.content.push(String::new());
                                }
                                if self.cursor_position.1 >= self.content.len() {
                                    self.cursor_position.1 = self.content.len() - 1;
                                }
                                self.cursor_position.0 = 0;
                                self.modified = true;
                                self.set_status_message("Line cut");
                            }
                            return Ok(());
                        }
                        (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                            if self.cursor_position.1 < self.content.len() {
                                let line = &self.content[self.cursor_position.1];
                                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                    match clipboard.set_text(line.clone()) {
                                        Ok(_) => self.set_status_message("Line copied"),
                                        Err(e) => self.set_status_message(&format!("Failed to copy: {}", e)),
                                    }
                                }
                            }
                            return Ok(());
                        }
                        (KeyCode::Char('v'), KeyModifiers::CONTROL) => {
                            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                match clipboard.get_text() {
                                    Ok(text) => {
                                        if self.cursor_position.1 < self.content.len() {
                                            let current_line = &mut self.content[self.cursor_position.1];
                                            current_line.insert_str(self.cursor_position.0, &text);
                                            self.cursor_position.0 += text.chars().count();
                                            self.modified = true;
                                            self.set_status_message("Pasted from clipboard");
                                        }
                                    }
                                    Err(e) => {
                                        self.set_status_message(&format!("Failed to paste: {}", e));
                                    }
                                }
                            } else {
                                self.set_status_message("Failed to access clipboard");
                            }
                            return Ok(());
                        }
                        (KeyCode::Char('z'), KeyModifiers::CONTROL) => {
                            self.undo();
                            return Ok(());
                        }
                        (KeyCode::Char('y'), KeyModifiers::CONTROL) => {
                            self.redo();
                            return Ok(());
                        }
                        (KeyCode::Char('f'), KeyModifiers::CONTROL) => {
                            self.popup_state = PopupType::Find;
                            self.search_query.clear();
                        }
                        (KeyCode::Char('r'), KeyModifiers::ALT) => {
                            if let Some(filename) = &self.filename {
                                let path = filename.to_str().unwrap_or("");
                                let run_command = if path.ends_with(".rs") {
                                    format!("cd '{}' && cargo run", std::env::current_dir().unwrap().display())
                                } else if path.ends_with(".cs") {
                                    format!("dotnet run '{}'", path)
                                } else if path.ends_with(".py") {
                                    format!("python3 '{}'", path)
                                } else {
                                    return Ok(());
                                };
                                terminal::disable_raw_mode()?;
                                crossterm::execute!(
                                    self.terminal.backend_mut(),
                                    terminal::LeaveAlternateScreen
                                )?;
                                let status = std::process::Command::new("sh")
                                    .arg("-c")
                                    .arg(&run_command)
                                    .status();
                                terminal::enable_raw_mode()?;
                                crossterm::execute!(
                                    self.terminal.backend_mut(),
                                    terminal::EnterAlternateScreen
                                )?;
                                self.draw()?;
                                match status {
                                    Ok(status) if status.success() => {
                                        self.set_status_message("Program ran successfully.");
                                    }
                                    Ok(status) => {
                                        self.set_status_message(format!("Program exited with status: {}", status));
                                    }
                                    Err(e) => {
                                        self.set_status_message(format!("Failed to run: {}", e));
                                    }
                                }
                                self.draw()?; // Refresh the canvas after running the program
                            }
                        }
                        (KeyCode::Char('r'), KeyModifiers::CONTROL) => {
                            self.search_query.clear();
                            self.mode = EditorMode::Replace;
                            self.popup_state = PopupType::Replace;
                        }
                        (KeyCode::Char('n'), KeyModifiers::ALT) => {
                            self.find_next();
                        }
                        (KeyCode::Char('e'), KeyModifiers::ALT) => {
                            if self.show_tree {
                                self.tree_focused = !self.tree_focused;
                                if self.tree_focused {
                                    self.file_entries = Self::read_directory(&self.current_dir)?;
                                }
                            }
                        }
                        (KeyCode::Char('t'), KeyModifiers::ALT) => {
                            self.popup_state = PopupType::ToolMenu;
                            self.tool_menu_selection = 0;
                        }
                        (KeyCode::Char('p'), KeyModifiers::ALT) => {
                            self.set_status_message("Settings not implemented yet");
                        }
                                (KeyCode::Char('h'), KeyModifiers::ALT) => {
                                    self.show_help();
                                }
                        (KeyCode::Char(c), _) => {
                            self.handle_text_input(c);
                        }
                        (KeyCode::Enter, _) => {
                            self.handle_enter_key();
                        }
                        (KeyCode::Backspace, _) => {
                            let delete_count = if key.modifiers.contains(KeyModifiers::SHIFT) { 5 } else { 1 };
                            for _ in 0..delete_count {
                                if self.cursor_position.0 > 0 {
                                    let current_line = &mut self.content[self.cursor_position.1];
                                    current_line.remove(self.cursor_position.0 - 1);
                                    self.cursor_position.0 -= 1;
                                    self.modified = true;
                                } else if self.cursor_position.1 > 0 {
                                    let _line = self.content.remove(self.cursor_position.1);
                                    self.cursor_position.1 -= 1;
                                    self.cursor_position.0 = self.content[self.cursor_position.1].len();
                                    self.content[self.cursor_position.1].push_str(&_line);
                                    self.modified = true;
                                }
                            }
                        }
                        (KeyCode::Esc, _) => {
                            self.has_edited = true;
                            self.popup_state = PopupType::InitialMenu;
                        }
                        _ => {}
                    }
                }
            }
            PopupType::JumpToLine => {
                match key.code {
                    KeyCode::Enter => {
                        if let Ok(line_num) = self.search_query.parse::<usize>() {
                            if line_num > 0 && line_num <= self.content.len() {
                                self.cursor_position.1 = line_num - 1;
                                self.cursor_position.0 = 0;
                                self.popup_state = PopupType::None;
                                self.search_query.clear();
                            } else {
                                self.set_status_message("Invalid line number");
                            }
                        } else {
                            self.set_status_message("Invalid line number");
                        }
                    }
                    KeyCode::Esc => {
                        self.popup_state = if self.show_initial_menu {
                            PopupType::InitialMenu
                        } else {
                            PopupType::None
                        };
                        self.search_query.clear();
                    }
                    KeyCode::Char(c) => {
                        if c.is_ascii_digit() {
                            self.search_query.push(c);
                        }
                    }
                    KeyCode::Backspace => {
                        if !self.search_query.is_empty() {
                            self.search_query.pop();
                        }
                    }
                    _ => {}
                }
            }
            PopupType::Replace => {
                self.popup_state = PopupType::ReplaceQuery;
                self.search_query.clear();
                self.replace_text.clear();
            }
            PopupType::ReplaceQuery => {
                match key.code {
                    KeyCode::Char(c) => {
                        self.search_query.push(c);
                    }
                    KeyCode::Backspace => {
                        self.search_query.pop();
                    }
                    KeyCode::Enter => {
                        self.find_next();
                        self.popup_state = PopupType::ReplaceWithQuery;
                    }
                    KeyCode::Esc => {
                        self.popup_state = PopupType::None;
                    }
                    _ => {}
                }
            }
            PopupType::ReplaceWithQuery => {
                match key.code {
                    KeyCode::Char(c) => {
                        self.replace_text.push(c);
                    }
                    KeyCode::Backspace => {
                        self.replace_text.pop();
                    }
                    KeyCode::Enter => {
                        if self.highlighted_matches.is_empty() {
                            self.set_status_message("No matches found.");
                        } else {
                            self.replace_current();
                            self.find_next();
                        }
                    }
                    KeyCode::Esc => {
                        self.popup_state = PopupType::None;
                    }
                    _ => {}
                }
            }
            PopupType::ToolMenu => {
                match key.code {
                    KeyCode::Up => {
                        self.tool_menu_selection = self.tool_menu_selection.saturating_sub(1);
                    }
                    KeyCode::Down => {
                        self.tool_menu_selection = (self.tool_menu_selection + 1) % self.tools.len();
                    }
                    KeyCode::Enter => {
                        self.handle_tool_menu_selection(self.tool_menu_selection);
                        self.popup_state = PopupType::None;
                    }
                    KeyCode::Esc => {
                        self.popup_state = PopupType::None;
                    }
                    _ => {}
                }
            }
            PopupType::NewFile => {
                match key.code {
                    KeyCode::Enter => {
                        self.create_new_file()?;
                    }
                    KeyCode::Esc => {
                        self.popup_state = PopupType::None;
                        self.temp_filename.clear();
                    }
                    KeyCode::Backspace => {
                        self.temp_filename.pop();
                    }
                    KeyCode::Char(c) => {
                        self.temp_filename.push(c);
                    }
                    _ => {}
                }
            }
            PopupType::NewDirectory => {
                match key.code {
                    KeyCode::Enter => {
                        self.create_new_directory()?;
                    }
                    KeyCode::Esc => {
                        self.popup_state = PopupType::None;
                        self.temp_filename.clear();
                    }
                    KeyCode::Backspace => {
                        self.temp_filename.pop();
                    }
                    KeyCode::Char(c) => {
                        self.temp_filename.push(c);
                    }
                    _ => {}
                }
            }
            PopupType::FileChanged => {
                match key.code {
                    KeyCode::Char('y') | KeyCode::Char('Y') => {
                        self.reload_file()?;
                        self.popup_state = PopupType::None;
                    }
                    KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                        self.popup_state = PopupType::None;
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
    fn cleanup(&mut self) -> std::io::Result<()> {
        terminal::disable_raw_mode()?;
        crossterm::execute!(
            self.terminal.backend_mut(),
            terminal::LeaveAlternateScreen
        )?;
        Ok(())
    }
    fn run_command(command: &str) -> Result<String, std::io::Error> {
        use std::process::Command;
        let output = Command::new("sh")
            .arg("-c")
            .arg(command)
            .output()?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if !stderr.is_empty() {
            Ok(stderr)
        } else {
            Ok(stdout)
        }
    }
    fn try_exit(&mut self) {
        if self.is_modified() {
            self.popup_state = PopupType::SaveConfirm(SaveAction::Exit);
        } else {
            self.cleanup().unwrap_or(());
            std::process::exit(0);
        }
    }
    fn handle_save_confirm(&mut self, save: bool, action: SaveAction) -> std::io::Result<()> {
        if save {
            if self.filename.is_none() {
                self.filename = Some(self.current_dir.join(&self.temp_filename));
                self.popup_state = PopupType::Save;
                return Ok(());
            }
            self.save()?;
        }
        match action {
            SaveAction::Exit => {
                if save {
                    self.cleanup()?;
                    std::process::exit(0);
                } else {
                    self.cleanup()?;
                    std::process::exit(0);
                }
            }
            SaveAction::OpenFile => {
                let path = PathBuf::from(&self.temp_filename);
                self.open_file(&path)?;
                self.temp_filename.clear();
                self.popup_state = PopupType::None;
            }
        }
        Ok(())
    }
    fn is_modified(&self) -> bool {
        if !self.modified {
            return false;
        }
        if let Some(path) = &self.filename {
            if let Ok(content) = fs::read_to_string(path) {
                let current = self.content.join("\n") + "\n";
                return current != content;
            }
        }
        true
    }
    fn safe_insert_char(&mut self, c: char) {
        if self.content.is_empty() {
            self.content.push(String::new());
        }
        self.cursor_position.1 = self.cursor_position.1.min(self.content.len() - 1);
        let current_line = &mut self.content[self.cursor_position.1];
        let mut chars: Vec<char> = current_line.chars().collect();
        self.cursor_position.0 = self.cursor_position.0.min(chars.len());
        let ascii_char = deunicode(&c.to_string());
        for ch in ascii_char.chars() {
            chars.insert(self.cursor_position.0, ch);
            self.cursor_position.0 += 1;
        }
        *current_line = chars.into_iter().collect();
        self.modified = true;
    }
    fn handle_text_input(&mut self, c: char) {
        if self.tree_focused {
            return;
        }
        if !c.is_control() {
            self.save_state();
            match c {
                '{' => self.insert_and_move_cursor("{}", 1),
                '(' => self.insert_and_move_cursor("()", 1),
                '[' => self.insert_and_move_cursor("[]", 1),
                '"' => self.insert_and_move_cursor("\"\"", 1),
                '\'' => self.insert_and_move_cursor("''", 1),
                _ => self.safe_insert_char(c),
            }
            if c.is_alphanumeric() || c == '_' || c == '.' {
                self.update_word_database();
                self.update_suggestions();
            } else {
                self.showing_suggestions = false;
            }
        }
    }
    fn insert_and_move_cursor(&mut self, text: &str, cursor_offset: usize) {
        if self.content.is_empty() {
            self.content.push(String::new());
        }
        self.cursor_position.1 = self.cursor_position.1.min(self.content.len() - 1);
        let current_line = &mut self.content[self.cursor_position.1];
        let mut chars: Vec<char> = current_line.chars().collect();
        self.cursor_position.0 = self.cursor_position.0.min(chars.len());
        for (i, ch) in text.chars().enumerate() {
            chars.insert(self.cursor_position.0 + i, ch);
        }
        *current_line = chars.into_iter().collect();
        self.cursor_position.0 += cursor_offset;
        self.modified = true;
    }
    fn get_char_position(&self, text: &str, byte_pos: usize) -> usize {
        text.chars()
            .take_while(|_| text[..byte_pos.min(text.len())].len() > 0)
            .count()
    }
    fn get_line_slice(line: &str, start: usize, end: usize) -> String {
        line.chars()
            .skip(start)
            .take(end.saturating_sub(start))
            .collect()
    }
    fn get_char_index(text: &str, byte_pos: usize) -> usize {
        text[..byte_pos.min(text.len())]
            .chars()
            .count()
    }
    fn ensure_cursor_in_bounds(&mut self) {
        if self.content.is_empty() {
            self.content.push(String::new());
        }
        let line = &self.content[self.cursor_position.1];
        let char_count = line.chars().count();
        self.cursor_position.0 = self.cursor_position.0.min(char_count);
    }
    fn find_next(&mut self) {
        if self.search_query.is_empty() {
            self.highlighted_matches.clear();
            return;
        }
        self.highlighted_matches.clear();
        for (line_idx, line) in self.content.iter().enumerate() {
            let mut start = 0;
            while let Some(pos) = line[start..].find(&self.search_query) {
                let abs_pos = start + pos;
                self.highlighted_matches.push((line_idx, abs_pos));
                start = abs_pos + 1;
            }
        }
        if !self.highlighted_matches.is_empty() {
            if let Some(search_index) = self.search_index {
                let next_index = (search_index + 1) % self.highlighted_matches.len();
                self.search_index = Some(next_index);
                let (line, col) = self.highlighted_matches[next_index];
                self.cursor_position = (col, line);
            } else {
                self.search_index = Some(0);
                let (line, col) = self.highlighted_matches[0];
                self.cursor_position = (col, line);
            }
        } else {
            self.search_index = None;
            self.set_status_message("No matches found");
        }
    }
    fn handle_search_input(&mut self, c: char) {
        match c {
            '\n' => self.find_next(),
            c if !c.is_control() => {
                self.search_query.push(c);
                self.find_next();
            }
            _ => {}
        }
    }
    fn open_file(&mut self, path: &PathBuf) -> std::io::Result<()> {
        if path.is_dir() {
            self.log_error(&format!("Attempted to open directory: {}", path.display()));
            self.set_status_message("Cannot open a directory");
            return Ok(());
        }
        if self.modified && self.filename.is_some() {
            self.save_state();
        }
        if let Some(index) = self.file_entries.iter().position(|entry| entry.path == *path) {
            self.file_explorer_selection = index;
        }
        match fs::read_to_string(path) {
            Ok(content) => {
                let ascii_content = content.lines()
                                           .map(|line| {
                                               if line.contains("󰆍") || line.contains("") {
                                                   line.to_string()
                                               } else {
                                                   deunicode(line)
                                               }
                                           })
                                           .collect::<Vec<String>>()
                                           .join("\n");
                self.content = ascii_content.lines().map(String::from).collect();
                if self.content.is_empty() {
                    self.content.push(String::new());
                }
                self.cursor_position = (0, 0);
                self.filename = Some(path.clone());
                self.modified = false;
                self.scroll_offset = 0;
                self.add_to_recent_files(path.clone());
                self.set_status_message(format!("Opened {}", Self::format_path(path)));
                self.show_initial_menu = false;
                self.current_syntax = Self::detect_syntax(&self.syntax_set, path);
                self.last_save_state = Some(self.content.clone());
                if let Some(index) = self.file_entries.iter().position(|entry| entry.path == *path) {
                    self.file_explorer_selection = index;
                }
                Ok(())
            }
            Err(e) => {
                let error_msg = format!("Error opening file: {}", e);
                self.log_error(&error_msg);
                self.set_status_message(&error_msg);
                return Ok(());
            }
        }
    }
    fn load_recent_files() -> Vec<RecentFile> {
        let home = env::var("HOME").ok().map(PathBuf::from);
        let config_dir = home.map(|h| h.join(".config").join("red"));
        if let Some(config_dir) = config_dir {
            if !config_dir.exists() {
                let _ = fs::create_dir_all(&config_dir);
            }
            let history_file = config_dir.join("history");
            if let Ok(content) = fs::read_to_string(history_file) {
                return content
                    .lines()
                    .filter_map(|line| {
                        let path = PathBuf::from(line);
                        Some(RecentFile {
                            exists: path.exists(),
                            last_modified: path.metadata().ok()?.modified().ok()?,
                            path,
                        })
                    })
                    .take(20)
                    .collect();
            }
        }
        Vec::new()
    }
    fn save_recent_files(&self) {
        if let Some(home) = env::var("HOME").ok().map(PathBuf::from) {
            let config_dir = home.join(".config").join("red");
            let _ = fs::create_dir_all(&config_dir);
            let history_file = config_dir.join("history");
            let content: String = self.recent_files
                .iter()
                .map(|rf| rf.path.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("\n");
            let _ = fs::write(history_file, content);
        }
    }
    fn add_to_recent_files(&mut self, path: PathBuf) {
        if let Some(existing) = self.recent_files
            .iter()
            .position(|rf| rf.path == path)
        {
            self.recent_files.remove(existing);
        }
        self.recent_files.insert(0, RecentFile {
            path,
            exists: true,
            last_modified: SystemTime::now(),
        });
        if self.recent_files.len() > 20 {
            self.recent_files.truncate(20);
        }
        self.save_recent_files();
    }
    fn read_directory(path: &Path) -> std::io::Result<Vec<FileEntry>> {
        Self::read_directory_with_depth(path, 0)
    }
    fn read_directory_with_depth(path: &Path, depth: usize) -> std::io::Result<Vec<FileEntry>> {
        let mut entries = Vec::new();
        if let Some(parent) = path.parent() {
            entries.push(FileEntry {
                name: String::from(".."),
                path: parent.to_path_buf(),
                is_dir: true,
                is_selected: false,
                depth,
            });
        }
        let mut dir_entries: Vec<_> = fs::read_dir(path)?
            .filter_map(|entry| entry.ok())
            .map(|entry| {
                let path = entry.path();
                let is_dir = path.is_dir();
                let name = path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                FileEntry {
                    name,
                    path,
                    is_dir,
                    is_selected: false,
                    depth,
                }
            })
            .collect();
        dir_entries.sort_by(|a, b| {
            match (a.is_dir, b.is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            }
        });
        entries.extend(dir_entries);
        Ok(entries)
    }
    fn get_file_icon(path: &Path) -> &'static str {
        if path.is_dir() {
            let name = path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            if name == ".." {
                return "";
            }
            for (folder_name, icon) in FOLDER_ICONS {
                if *folder_name == "" || name.to_lowercase() == *folder_name {
                    return icon;
                }
            }
            return "";
        }
        let ext = path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        match name.to_lowercase().as_str() {
            "dockerfile" => return "",
            "docker-compose.yml" | "docker-compose.yaml" => return "",
            "package.json" => return "",
            "cargo.toml" => return "",
            "makefile" => return "",
            "readme.md" => return "",
            "license" => return "",
            ".env" => return "",
            _ => {}
        }
        for (extension, icon) in FILE_ICONS {
            if *extension == "" {
                continue;
            }
            if ext.to_lowercase() == *extension {
                return icon;
            }
        }
        ""
    }
    fn truncate_to_width(text: &str, width: u16) -> String {
        let mut length = 0;
        let mut result = String::new();
        for c in text.chars() {
            let char_width = if c.is_ascii() { 1 } else { 2 };
            if length + char_width > width as usize {
                break;
            }
            length += char_width;
            result.push(c);
        }
        result
    }
    fn detect_syntax(syntax_set: &SyntaxSet, path: &Path) -> Option<String> {
        if let Some(syntax) = syntax_set.find_syntax_for_file(path).ok()? {
            Some(syntax.name.clone())
        } else {
            path.extension()
                .and_then(|ext| ext.to_str())
                .and_then(|ext| {
                    match ext {
                        "py" => Some("Python"),
                        "rs" => Some("Rust"),
                        "js" => Some("JavaScript"),
                        "jsx" => Some("JavaScript (JSX)"),
                        "ts" => Some("TypeScript"),
                        "tsx" => Some("TypeScript (JSX)"),
                        "cpp" | "cc" | "cxx" => Some("C++"),
                        "c" | "h" => Some("C"),
                        "go" => Some("Go"),
                        "html" => Some("HTML"),
                        "css" => Some("CSS"),
                        "sh" => Some("Shell"),
                        _ => None,
                    }
                })
                .map(String::from)
        }
    }
    fn update_word_database(&mut self) {
        let mut word_weights = HashMap::new();
        for line in &self.content {
            for word in line.split_whitespace() {
                if word.len() > 2 && !word.chars().all(|c| c.is_numeric()) {
                    *word_weights.entry(word.to_string()).or_insert(0.0) += 1.0;
                }
            }
        }
        for keyword in &self.language_keywords {
            word_weights.insert(keyword.clone(), 2.0);
        }
        self.word_database = word_weights;
    }
    fn get_current_word(&self) -> Option<(String, usize)> {
        if self.cursor_position.1 >= self.content.len() {
            return None;
        }
        let line = &self.content[self.cursor_position.1];
        if line.is_empty() || self.cursor_position.0 == 0 {
            return None;
        }
        let before_cursor = &line[..self.cursor_position.0];
        let word_start = before_cursor.rfind(|c: char| !c.is_alphanumeric() && c != '_' && c != '.')
            .map(|i| i + 1)
            .unwrap_or(0);
        if word_start == self.cursor_position.0 {
            return None;
        }
        Some((
            before_cursor[word_start..].to_string(),
            word_start
        ))
    }
    fn update_suggestions(&mut self) {
        if let Some((current_word, _)) = self.get_current_word() {
            if current_word.len() < 1 {
                self.showing_suggestions = false;
                return;
            }
            let suggestions = if let Some(syntax_name) = &self.current_syntax {
                self.get_language_suggestions(syntax_name, &current_word)
            } else {
                Vec::new()
            };
            self.suggestions = suggestions;
            self.showing_suggestions = !self.suggestions.is_empty();
            self.suggestion_index = 0;
        } else {
            self.showing_suggestions = false;
            self.suggestions.clear();
        }
    }
    fn get_language_suggestions(&self, syntax_name: &str, word: &str) -> Vec<String> {
        let suggestions = match syntax_name {
            "Rust" => vec![
                "fn", "let", "mut", "pub", "use", "struct", "enum", "impl", "trait", "match", "if", "else", "while",
                "for", "loop", "return", "break", "continue", "where", "type", "const", "static", "unsafe", "extern",
                "super", "self", "crate", "mod", "as", "in", "move", "box", "ref", "async", "await", "dyn", "macro_rules",
                "fn main() {\n    \n}", "let mut ", "println!(\"{}\", )", "#[derive(Debug)]", "Option<>", "Result<, >",
                "Vec::new()", "String::from()", "HashMap::new()", "#[derive(Clone)]", "#[derive(Default)]",
                "impl Default for ", "impl From<> for ", "impl Into<> for ", "#[cfg(test)]", "#[test]",
                "Clone", "Debug", "Default", "PartialEq", "Eq", "PartialOrd", "Ord", "Hash", "Display", "Error",
                "From", "Into", "AsRef", "AsMut", "Deref", "DerefMut", "Drop", "Send", "Sync", "Sized", "Copy",
                "ToOwned", "Borrow", "BorrowMut", "Iterator", "IntoIterator", "FromIterator", "Extend",
                "assert!", "assert_eq!", "assert_ne!", "panic!", "vec!", "format!", "write!", "include_str!",
                "include_bytes!", "concat!", "env!", "file!", "line!", "column!", "stringify!", "cfg!", "todo!",
                "String", "Vec", "HashMap", "HashSet", "BTreeMap", "BTreeSet", "Box", "Rc", "Arc", "Mutex",
                "RwLock", "Cell", "RefCell", "Cow", "Pin", "PhantomData", "Duration", "Instant", "SystemTime",
                "unwrap()", "expect()", "unwrap_or()", "unwrap_or_else()", "unwrap_or_default()",
                "map()", "and_then()", "or_else()", "filter()", "fold()", "collect()", "iter()", "into_iter()",
                "to_string()", "to_owned()", "as_ref()", "as_mut()", "as_slice()", "as_str()", "parse()",
                "clone()", "is_some()", "is_none()", "is_ok()", "is_err()", "contains()", "insert()", "remove()",
                "async move", "tokio::spawn", "tokio::main", "futures::StreamExt", "futures::SinkExt",
                "async fn handle_connection", "async_trait", "select!", "join!", "spawn_blocking",
                "#[test]\nfn test_() {\n    \n}", "#[bench]", "#[should_panic]", "#[ignore]",
                "assert!()", "assert_eq!()", "assert_ne!()", "dbg!()", "#[cfg(test)]\nmod tests {\n    \n}",
                "Result<(), Error>", "anyhow::Result<()>", "thiserror::Error", "Box<dyn Error>",
                "#[derive(Error)]\n#[error(\"\")]", "bail!()", "ensure!()", "Ok(())", "Err(anyhow!())",
                "reqwest::Client", "tokio::net::TcpListener", "tokio::net::TcpStream", "hyper::Server",
                "warp::Filter", "actix_web::HttpResponse", "rocket::get", "async_std::net",
                "std::fs::File", "std::io::BufReader", "std::io::BufWriter", "std::path::PathBuf",
                "tokio::fs::read_to_string", "tokio::io::AsyncReadExt", "tokio::io::AsyncWriteExt",
                "serde::Serialize", "serde::Deserialize", "#[derive(Serialize)]", "#[derive(Deserialize)]",
                "serde_json::to_string", "serde_json::from_str", "toml::to_string", "toml::from_str",
                "while true {\n    \n}", "for i in 0..10 {\n    \n}", "loop {\n    \n}",
                "match value {\n    Some(v) => ,\n    None => ,\n}",
                "if let Some(value) = option {\n    \n}",
                "while let Some(value) = iter.next() {\n    \n}"
            ],
            "Python" => vec![
                "def", "class", "if", "else", "elif", "while", "for", "in", "try", "except", "finally", "with",
                "import", "from", "as", "return", "yield", "lambda", "raise", "assert", "global", "nonlocal",
                "pass", "break", "continue", "del", "is", "not", "and", "or", "async", "await", "property",
                "def __init__(self):", "if __name__ == '__main__':", "def __str__(self):", "def __repr__(self):",
                "class Meta:", "def setUp(self):", "def tearDown(self):", "def test_", "async def ",
                "@property", "@classmethod", "@staticmethod", "@abstractmethod", "@contextmanager",
                "@dataclass", "@cached_property", "@lru_cache", "@singledispatch", "@wraps",
                "print()", "len()", "range()", "enumerate()", "zip()", "map()", "filter()", "reduce()",
                "sorted()", "reversed()", "sum()", "min()", "max()", "abs()", "round()", "isinstance()",
                "issubclass()", "hasattr()", "getattr()", "setattr()", "delattr()", "callable()",
                "import numpy as np", "import pandas as pd", "import matplotlib.pyplot as plt",
                "import tensorflow as tf", "import torch", "import sklearn", "import requests",
                "import json", "import os", "import sys", "import datetime", "import logging",
                "import argparse", "import unittest", "import pytest", "import asyncio",
                "list()", "dict()", "set()", "tuple()", "frozenset()", "collections.defaultdict()",
                "collections.Counter()", "collections.deque()", "collections.namedtuple()",
                "with open('', 'r') as f:", "with open('', 'w') as f:", "with open('', 'rb') as f:",
                "os.path.join()", "os.path.exists()", "os.makedirs()", "os.remove()", "shutil.copy()",
                "try:\n    \nexcept Exception as e:", "raise ValueError()", "raise TypeError()",
                "raise NotImplementedError()", "raise RuntimeError()", "finally:", "else:",
                "def test_():", "assert ", "self.assertEqual()", "self.assertTrue()", "self.assertFalse()",
                "self.assertRaises()", "pytest.fixture", "@pytest.mark.parametrize",
                "requests.get()", "requests.post()", "requests.put()", "requests.delete()",
                "flask.Flask(__name__)", "@app.route('/')", "django.urls.path",
                "cursor.execute()", "connection.commit()", "Session()", "Model.query.all()",
                "Model.query.filter_by()", "db.Column()", "db.relationship()",
                "while True:\n    ", "for i in range():\n    ", "for item in items:\n    ",
                "if condition:\n    \nelse:\n    ", "try:\n    \nexcept:\n    \nfinally:\n    ",
                "def function():\n    return", "class ClassName:\n    def __init__(self):\n        "
            ],
            "JavaScript" => vec![
                "function", "const", "let", "var", "class", "if", "else", "for", "while", "do", "switch",
                "case", "break", "continue", "return", "try", "catch", "finally", "throw", "typeof",
                "instanceof", "new", "this", "super", "extends", "static", "get", "set", "async", "await",
                "yield", "delete", "void", "default", "debugger", "export", "import", "in", "of",
                "function() {\n    \n}", "() => {\n    \n}", "class extends {\n    constructor() {\n        super();\n    }\n}",
                "async function() {\n    \n}", "for (let i = 0; i < ; i++)", "for (const of )",
                "document.querySelector()", "document.getElementById()", "document.createElement()",
                "element.addEventListener()", "element.removeEventListener()", "element.innerHTML",
                "element.textContent", "element.classList.add()", "element.classList.remove()",
                "Array.isArray()", "Object.keys()", "Object.values()", "Object.entries()",
                "JSON.stringify()", "JSON.parse()", "Math.floor()", "Math.ceil()", "Math.round()",
                "String.prototype.trim()", "Array.prototype.map()", "Array.prototype.filter()",
                "new Promise((resolve, reject) => )", "Promise.all()", "Promise.race()",
                "Promise.resolve()", "Promise.reject()", "async/await", "try/catch",
                "localStorage.getItem()", "localStorage.setItem()", "sessionStorage.getItem()",
                "fetch()", "WebSocket", "requestAnimationFrame()", "setTimeout()", "setInterval()",
                "...spread", "destructuring", "optional?.chaining", "nullish??coalescing",
                "Array.prototype.flatMap()", "Object.fromEntries()", "globalThis",
                "describe('', () => )", "it('', () => )", "test('', () => )", "expect().toBe()",
                "beforeEach(() => )", "afterEach(() => )", "jest.mock()", "jest.spyOn()",
                "import React from 'react'", "useState", "useEffect", "useContext", "useRef",
                "useCallback", "useMemo", "useReducer", "const [state, setState] = useState()",
                "require()", "module.exports", "process.env", "Buffer.from()", "fs.readFile()",
                "path.join()", "http.createServer()", "express()", "app.get()", "app.post()",
                "while (condition) {\n    \n}", "for (let i = 0; i < length; i++) {\n    \n}",
                "do {\n    \n} while (condition);", "if (condition) {\n    \n} else {\n    \n}",
                "switch (value) {\n    case x:\n        break;\n    default:\n        break;\n}",
                "try {\n    \n} catch (error) {\n    \n} finally {\n    \n}"
            ],
            "C#" => vec![
                "public", "private", "protected", "internal", "class", "interface", "struct", "enum",
                "static", "readonly", "const", "async", "await", "using", "namespace", "var",
                "public class  {\n    \n}", "public static void Main(string[] args) {\n    \n}",
                "public async Task  {\n    \n}", "try {\n    \n} catch (Exception ex) {\n    \n}",
                "[Serializable]\npublic class ",
                "Console.WriteLine()", "Console.Write()", "Console.ReadLine()", "List<>", "Dictionary<, >", "IEnumerable<>",
                "string.Format()", "StringBuilder", "Task.Run(async () => )", "await Task.WhenAll()",
                "Enumerable.Range(0, 10).Select(x => x * 2)", "Enumerable.Empty<int>()",
                "Enumerable.Repeat(0, 10)", "Enumerable.Concat()", "Enumerable.Zip()",
                "[Obsolete]", "[Serializable]", "[NonSerialized]", "[DllImport]",
                "try {\n    \n} catch (Exception ex) {\n    \n}", "throw new Exception()",
                "throw new ArgumentNullException()", "throw new InvalidOperationException()",
                "File.ReadAllText()", "File.WriteAllText()", "FileStream", "StreamReader", "StreamWriter",
                "Task.Delay()", "Task.WhenAll()", "Task.WhenAny()", "CancellationToken",
                "CancellationTokenSource", "SemaphoreSlim", "Mutex", "Monitor",
                "[TestMethod]", "[TestClass]", "Assert.AreEqual()", "Assert.IsTrue()", "Assert.IsFalse()",
                "Assert.ThrowsException<>()", "Moq.Mock<>", "Moq.It.IsAny<>", "Moq.It.Is<>",
                "HttpClient", "HttpRequestMessage", "HttpResponseMessage", "HttpClient.GetAsync()",
                "HttpClient.PostAsync()", "HttpClient.PutAsync()", "HttpClient.DeleteAsync()",
                "JsonConvert.SerializeObject()", "JsonConvert.DeserializeObject<>",
                "XmlSerializer", "DataContractSerializer", "BinaryFormatter",
                "while () {\n    \n}", "for (int i = 0; i < length; i++) {\n    \n}",
                "foreach (var item in collection) {\n    \n}", "do {\n    \n} while ();",
                "if () {\n    \n} else {\n    \n}", "switch () {\n    case :\n        break;\n    default:\n        break;\n}",
                "using (var resource = new Resource()) {\n    \n}",
                "lock (lockObject) {\n    \n}", "try {\n    \n} catch {\n    \n} finally {\n    \n}"
            ],
            "Java" => vec![
                "public", "private", "protected", "class", "interface", "enum", "extends", "implements",
                "static", "final", "abstract", "synchronized", "volatile", "transient", "native", "strictfp",
                "import", "package", "new", "return", "void", "int", "long", "double", "float", "boolean",
                "char", "byte", "short", "null", "true", "false", "if", "else", "switch", "case", "default",
                "for", "while", "do", "break", "continue", "try", "catch", "finally", "throw", "throws",
                "this", "super", "instanceof", "assert", "goto", "const",
                "public class  {\n    \n}", "public static void main(String[] args) {\n    \n}",
                "public void () {\n    \n}", "try {\n    \n} catch (Exception e) {\n    \n}",
                "@Override\npublic void ",
                "System.out.println()", "System.err.println()", "List<>", "Map<, >", "Set<>",
                "ArrayList<>()", "HashMap<>()", "HashSet<>()", "Collections.sort()", "Collections.emptyList()",
                "try {\n    \n} catch (Exception e) {\n    \n}", "throw new Exception()",
                "throw new IllegalArgumentException()", "throw new NullPointerException()",
                "FileReader", "FileWriter", "BufferedReader", "BufferedWriter", "InputStream",
                "OutputStream", "FileInputStream", "FileOutputStream",
                "Thread", "Runnable", "Callable", "ExecutorService", "Executors.newFixedThreadPool()",
                "Executors.newSingleThreadExecutor()", "Future", "CountDownLatch", "Semaphore", "ReentrantLock",
                "@Test", "Assert.assertEquals()", "Assert.assertTrue()", "Assert.assertFalse()",
                "Assert.assertThrows()", "Mockito.mock()", "Mockito.when()", "Mockito.verify()",
                "HttpURLConnection", "URLConnection", "URL", "HttpClient", "HttpRequest", "HttpResponse",
                "ObjectOutputStream", "ObjectInputStream", "Serializable", "Externalizable",
                "Gson.toJson()", "Gson.fromJson()",
                "while () {\n    \n}", "for (int i = 0; i < length; i++) {\n    \n}",
                "for (Type item : collection) {\n    \n}", "do {\n    \n} while ();",
                "if () {\n    \n} else {\n    \n}", "switch () {\n    case :\n        break;\n    default:\n        break;\n}",
                "synchronized () {\n    \n}", "try {\n    \n} catch (Exception e) {\n    \n} finally {\n    \n}"
            ],
            _ => vec![],
        };
        suggestions.into_iter()
            .filter(|s| s.starts_with(word))
            .map(String::from)
            .collect()
    }
    fn apply_suggestion(&mut self) {
        if !self.showing_suggestions || self.suggestions.is_empty() {
            return;
        }
        if let Some((_, word_start)) = self.get_current_word() {
            let suggestion = &self.suggestions[self.suggestion_index];
            let line = &mut self.content[self.cursor_position.1];
            if suggestion.contains('\n') {
                let indent = line.chars().take_while(|c| c.is_whitespace()).collect::<String>();
                let lines: Vec<String> = suggestion
                    .lines()
                    .enumerate()
                    .map(|(i, l)| {
                        if i == 0 {
                            l.to_string()
                        } else {
                            format!("{}{}", indent, l)
                        }
                    })
                    .collect();
                line.replace_range(word_start..self.cursor_position.0, &lines[0]);
                if lines.len() > 1 {
                    for (i, new_line) in lines.into_iter().skip(1).enumerate() {
                        self.content.insert(self.cursor_position.1 + i + 1, new_line);
                    }
                }
            } else {
                line.replace_range(word_start..self.cursor_position.0, suggestion);
                self.cursor_position.0 = word_start + suggestion.len();
            }
            self.modified = true;
        }
        self.showing_suggestions = false;
    }
    fn get_word_start(&self, line: &str, cursor_x: usize) -> usize {
        let count = line[..cursor_x].chars().rev()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .count();
        cursor_x.saturating_sub(count)
    }
    fn centered_rect(&self, width: u16, height: u16, r: Rect) -> Rect {
        let popup_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length((r.height.saturating_sub(height)) / 2),
                Constraint::Length(height),
                Constraint::Min(0)
            ])
            .split(r);
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length((r.width.saturating_sub(width)) / 2),
                Constraint::Length(width),
                Constraint::Min(0)
            ])
            .split(popup_layout[1])[1]
    }
    fn update_word_database_for_syntax(&mut self, syntax_name: &str) {
        self.language_keywords.clear();
        let keywords = match syntax_name {
            "Python" => vec![
                ("if", 2.0), ("else:", 2.0), ("elif", 2.0), ("while", 2.0), ("for", 2.0),
                ("def", 2.0), ("class", 2.0), ("return", 2.0), ("import", 2.0), ("from", 2.0),
                ("try", 2.0), ("except", 2.0), ("finally", 2.0), ("raise", 2.0), ("with", 2.0),
                ("as", 2.0), ("in", 2.0), ("is", 2.0), ("not", 2.0), ("and", 2.0), ("or", 2.0),
                ("lambda", 2.0), ("yield", 2.0), ("async", 2.0), ("await", 2.0), ("break", 2.0),
                ("continue", 2.0), ("pass", 2.0), ("assert", 2.0), ("del", 2.0), ("global", 2.0),
                ("if :\n    ", 2.5),
                ("while :\n    ", 2.5),
                ("for  in :\n    ", 2.5),
                ("def ():\n    ", 2.5),
                ("class ():\n    ", 2.5),
                ("try:\n    \nexcept Exception as e:\n    ", 2.5),
                ("async def ():\n    ", 2.5),
                ("@property\ndef (self):\n    ", 2.5),
                ("@classmethod\ndef (cls):\n    ", 2.5),
                ("@staticmethod\ndef ():\n    ", 2.5),
                ("print()", 2.0), ("len()", 2.0), ("range()", 2.0), ("str()", 2.0),
                ("int()", 2.0), ("list()", 2.0), ("dict()", 2.0), ("set()", 2.0),
                ("tuple()", 2.0), ("float()", 2.0), ("bool()", 2.0), ("bytes()", 2.0),
                ("map()", 2.0), ("filter()", 2.0), ("zip()", 2.0), ("enumerate()", 2.0),
                ("sorted()", 2.0), ("reversed()", 2.0), ("sum()", 2.0), ("any()", 2.0),
                ("all()", 2.0), ("min()", 2.0), ("max()", 2.0), ("abs()", 2.0),
                ("import os", 2.0), ("import sys", 2.0), ("import json", 2.0),
                ("from datetime import datetime", 2.0), ("import re", 2.0),
                ("import pathlib", 2.0), ("import requests", 2.0), ("import numpy as np", 2.0),
                ("import pandas as pd", 2.0), ("import matplotlib.pyplot as plt", 2.0),
                ("from typing import List, Dict, Tuple, Optional", 2.0),
                ("if __name__ == '__main__':", 2.0),
                ("with open() as f:", 2.0),
                ("def __init__(self):\n    ", 2.0),
                ("def __str__(self):\n    ", 2.0),
                ("def __repr__(self):\n    ", 2.0),
                ("def __len__(self):\n    ", 2.0),
                ("def __getitem__(self, key):\n    ", 2.0),
            ],
            "Rust" => vec![
                ("fn", 2.0), ("let", 2.0), ("mut", 2.0), ("pub", 2.0), ("use", 2.0),
                ("struct", 2.0), ("enum", 2.0), ("impl", 2.0), ("trait", 2.0), ("type", 2.0),
                ("mod", 2.0), ("crate", 2.0), ("super", 2.0), ("self", 2.0), ("Self", 2.0),
                ("where", 2.0), ("async", 2.0), ("await", 2.0), ("move", 2.0), ("static", 2.0),
                ("const", 2.0), ("extern", 2.0), ("unsafe", 2.0), ("dyn", 2.0),
                ("fn main() {\n    \n}", 2.5),
                ("if  {\n    \n}", 2.5),
                ("while  {\n    \n}", 2.5),
                ("for  in  {\n    \n}", 2.5),
                ("match  {\n    _ => \n}", 2.5),
                ("struct  {\n    \n}", 2.5),
                ("impl  {\n    \n}", 2.5),
                ("enum  {\n    \n}", 2.5),
                ("trait  {\n    \n}", 2.5),
                ("async fn  {\n    \n}", 2.5),
                ("#[derive(Debug)]\n", 2.5),
                ("#[derive(Clone, Copy)]\n", 2.5),
                ("#[derive(PartialEq, Eq)]\n", 2.5),
                ("println!()", 2.0), ("eprintln!()", 2.0), ("format!()", 2.0),
                ("Vec::new()", 2.0), ("vec![]", 2.0), ("vec![", 2.0),
                ("String::from()", 2.0), ("String::new()", 2.0), ("to_string()", 2.0),
                ("Option<>", 2.0), ("Some()", 2.0), ("None", 2.0),
                ("Result<, >", 2.0), ("Ok()", 2.0), ("Err()", 2.0),
                ("Box::new()", 2.0), ("Rc::new()", 2.0), ("Arc::new()", 2.0),
                ("HashMap::new()", 2.0), ("BTreeMap::new()", 2.0),
                ("HashSet::new()", 2.0), ("BTreeSet::new()", 2.0),
            ],
            "JavaScript" => vec![
                ("function", 2.0), ("const", 2.0), ("let", 2.0), ("var", 2.0), ("class", 2.0),
                ("if", 2.0), ("else", 2.0), ("for", 2.0), ("while", 2.0), ("do", 2.0),
                ("try", 2.0), ("catch", 2.0), ("finally", 2.0), ("throw", 2.0),
                ("async", 2.0), ("await", 2.0), ("import", 2.0), ("export", 2.0),
                ("function () {\n    \n}", 2.5),
                ("() => {\n    \n}", 2.5),
                ("class  {\n    constructor() {\n        \n    }\n}", 2.5),
                ("if () {\n    \n}", 2.5),
                ("for (let i = 0; i < ; i++) {\n    \n}", 2.5),
                ("try {\n    \n} catch (error) {\n    \n}", 2.5),
                ("import { } from '';", 2.5),
                ("export const  = ", 2.5),
                ("console.log()", 2.0), ("console.error()", 2.0),
                ("setTimeout(() => , )", 2.0), ("setInterval(() => , )", 2.0),
                ("Promise.resolve()", 2.0), ("Promise.reject()", 2.0),
                ("Array.from()", 2.0), ("Object.keys()", 2.0), ("Object.values()", 2.0),
                ("map()", 2.0), ("filter()", 2.0), ("reduce()", 2.0), ("forEach()", 2.0),
                ("includes()", 2.0), ("indexOf()", 2.0), ("join()", 2.0), ("split()", 2.0),
            ],
            "TypeScript" => vec![
                ("interface", 2.0), ("type", 2.0), ("enum", 2.0), ("namespace", 2.0),
                ("readonly", 2.0), ("private", 2.0), ("public", 2.0), ("protected", 2.0),
                ("implements", 2.0), ("extends", 2.0), ("abstract", 2.0), ("declare", 2.0),
                (": string", 2.0), (": number", 2.0), (": boolean", 2.0),
                (": any", 2.0), (": void", 2.0), (": never", 2.0),
                (": Record<, >", 2.0), (": Partial<>", 2.0), (": Readonly<>", 2.0),
                ("interface  {\n    \n}", 2.5),
                ("type  = ", 2.5),
                ("enum  {\n    \n}", 2.5),
                ("class  implements  {\n    \n}", 2.5),
                ("function <T>(): T {\n    \n}", 2.5),
            ],
            "C++" => vec![
                ("class", 2.0), ("struct", 2.0), ("template", 2.0), ("typename", 2.0),
                ("public", 2.0), ("private", 2.0), ("protected", 2.0), ("virtual", 2.0),
                ("const", 2.0), ("static", 2.0), ("inline", 2.0), ("namespace", 2.0),
                ("int main() {\n    \n    return 0;\n}", 2.5),
                ("class  {\npublic:\n    \n};", 2.5),
                ("template<typename T>\n", 2.5),
                ("namespace  {\n    \n}", 2.5),
                ("try {\n    \n} catch (const std::exception& e) {\n    \n}", 2.5),
                ("#include <iostream>", 2.0), ("#include <string>", 2.0),
                ("#include <vector>", 2.0), ("#include <map>", 2.0),
                ("using namespace std;", 2.0), ("using std::string;", 2.0),
                ("std::cout << ", 2.0), ("std::endl", 2.0),
                ("std::vector<>", 2.0), ("std::string", 2.0),
                ("std::map<, >", 2.0), ("std::shared_ptr<>", 2.0),
            ],
            "Go" => vec![
                ("func", 2.0), ("type", 2.0), ("struct", 2.0), ("interface", 2.0),
                ("var", 2.0), ("const", 2.0), ("package", 2.0), ("import", 2.0),
                ("go", 2.0), ("chan", 2.0), ("defer", 2.0), ("select", 2.0),
                ("func main() {\n    \n}", 2.5),
                ("func () error {\n    \n}", 2.5),
                ("type  struct {\n    \n}", 2.5),
                ("if err != nil {\n    return err\n}", 2.5),
                ("for _, v := range  {\n    \n}", 2.5),
                ("fmt.Println()", 2.0), ("fmt.Printf()", 2.0),
                ("make()", 2.0), ("new()", 2.0), ("append()", 2.0),
                ("len()", 2.0), ("cap()", 2.0), ("close()", 2.0),
                ("errors.New()", 2.0), ("panic()", 2.0), ("recover()", 2.0),
            ],
            "Java" => vec![
                ("public", 2.0), ("private", 2.0), ("protected", 2.0), ("class", 2.0),
                ("interface", 2.0), ("extends", 2.0), ("implements", 2.0),
                ("static", 2.0), ("final", 2.0), ("abstract", 2.0), ("synchronized", 2.0),
                ("public class  {\n    \n}", 2.5),
                ("public static void main(String[] args) {\n    \n}", 2.5),
                ("public void () {\n    \n}", 2.5),
                ("try {\n    \n} catch (Exception e) {\n    \n}", 2.5),
                ("@Override\npublic void ", 2.5),
                ("import java.util.*;", 2.0), ("import java.io.*;", 2.0),
                ("System.out.println()", 2.0), ("System.err.println()", 2.0),
                ("List<>", 2.0), ("Map<, >", 2.0), ("Set<>", 2.0),
                ("ArrayList<>()", 2.0), ("HashMap<>()", 2.0),
            ],
            "C#" => vec![
                ("public", 2.0), ("private", 2.0), ("protected", 2.0), ("internal", 2.0),
                ("class", 2.0), ("interface", 2.0), ("struct", 2.0), ("enum", 2.0),
                ("static", 2.0), ("readonly", 2.0), ("const", 2.0), ("async", 2.0),
                ("await", 2.0), ("using", 2.0), ("namespace", 2.0), ("var", 2.0),
                ("public class  {\n    \n}", 2.5),
                ("public static void Main(string[] args) {\n    \n}", 2.5),
                ("public async Task  {\n    \n}", 2.5),
                ("try {\n    \n} catch (Exception ex) {\n    \n}", 2.5),
                ("[Serializable]\npublic class ", 2.5),
                ("Console.WriteLine()", 2.0), ("Console.ReadLine()", 2.0),
                ("List<>", 2.0), ("Dictionary<, >", 2.0), ("IEnumerable<>", 2.0),
                ("string.Format()", 2.0), ("StringBuilder", 2.0),
                ("Task.Run(async () => )", 2.0), ("await Task.WhenAll()", 2.0),
            ],
            "PHP" => vec![
                ("function", 2.0), ("class", 2.0), ("public", 2.0), ("private", 2.0),
                ("protected", 2.0), ("static", 2.0), ("namespace", 2.0), ("use", 2.0),
                ("require", 2.0), ("include", 2.0), ("echo", 2.0), ("return", 2.0),
                ("<?php\n\n", 2.5),
                ("function () {\n    \n}", 2.5),
                ("class  {\n    \n}", 2.5),
                ("try {\n    \n} catch (Exception $e) {\n    \n}", 2.5),
                ("array()", 2.0), ("strlen()", 2.0), ("count()", 2.0),
                ("json_encode()", 2.0), ("json_decode()", 2.0),
                ("mysqli_query()", 2.0), ("PDO::prepare()", 2.0),
            ],
            "Ruby" => vec![
                ("def", 2.0), ("class", 2.0), ("module", 2.0), ("attr_accessor", 2.0),
                ("require", 2.0), ("include", 2.0), ("extend", 2.0), ("private", 2.0),
                ("def initialize\n    \nend", 2.5),
                ("class  < ApplicationRecord\n    \nend", 2.5),
                ("module \n    \nend", 2.5),
                ("begin\n    \nrescue => e\n    \nend", 2.5),
                ("puts ", 2.0), ("print ", 2.0), ("gets.chomp", 2.0),
                ("each do ||\n    \nend", 2.0), ("map { || }", 2.0),
            ],
            _ => vec![],
        };
        let mut weighted_keywords = HashMap::new();
        for (keyword, weight) in keywords {
            weighted_keywords.insert(keyword.to_string(), weight);
        }
        self.word_database = weighted_keywords;
    }
    fn draw_help(frame: &mut Frame, help_text: &[(&str, &str, &str)], scroll_offset: u16) {
        let area = frame.size();
        let width = area.width.saturating_sub(4).min(100);
        let height = area.height.saturating_sub(4);
        let help_area = Rect::new(
            (area.width.saturating_sub(width)) / 2,
            (area.height.saturating_sub(height)) / 2,
            width,
            height
        );
        frame.render_widget(Clear, help_area);
        let help_block = Block::default()
            .title(" Keyboard Shortcuts (Use ↑↓ to scroll) ")
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));
        let inner = help_block.inner(help_area);
        frame.render_widget(help_block, help_area);
        let mut text = Vec::new();
        let max_key_width = 12;
        let max_action_width = 15;
        let desc_width = inner.width.saturating_sub(max_key_width as u16 + max_action_width as u16 + 6);
        for (key, action, desc) in help_text {
            if action.is_empty() {
                if !text.is_empty() {
                    text.push(Line::from(""));
                }
                text.push(Line::from(vec![
                    Span::styled(
                        format!("─── {} ", key),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    ),
                    Span::styled(
                        "─".repeat((desc_width as usize).saturating_sub(key.len() + 4)),
                        Style::default().fg(Color::DarkGray)
                    ),
                ]));
                continue;
            }
            text.push(Line::from(vec![
                Span::styled(
                    format!("{:width$}", key, width = max_key_width),
                    Style::default().fg(Color::Green)
                ),
                Span::raw(" "),
                Span::styled(
                    format!("{:width$}", action, width = max_action_width),
                    Style::default().fg(Color::White)
                ),
                Span::raw(" "),
                Span::styled(
                    desc.to_string(),
                    Style::default().fg(Color::Gray)
                )
            ]));
        }
        let help_text = Paragraph::new(text)
            .alignment(Alignment::Left)
            .scroll((scroll_offset, 0));
        frame.render_widget(help_text, inner);
    }
    fn try_close_tab(&mut self) {
        if self.modified {
            self.popup_state = PopupType::SaveConfirm(SaveAction::Exit);
        } else {
            if self.tabs.len() > 1 {
                self.tabs.remove(self.active_tab);
                if self.active_tab >= self.tabs.len() {
                    self.active_tab = self.tabs.len() - 1;
                }
            } else {
                self.cleanup().unwrap_or(());
                std::process::exit(0);
            }
        }
    }
    fn enter_directory(&mut self, path: PathBuf, new_depth: usize) -> std::io::Result<()> {
        self.current_dir = path;
        self.file_entries = Self::read_directory_with_depth(&self.current_dir, new_depth)?;
        self.file_explorer_selection = 0;
        Ok(())
    }
    fn log_error(&self, error: &str) {
        if let Some(home) = env::var("HOME").ok().map(PathBuf::from) {
            let log_dir = home.join(".config").join("red").join("logs");
            let _ = fs::create_dir_all(&log_dir);
            let timestamp = Local::now().format("%Y-%m-%d_%H-%M-%S");
            let log_file = log_dir.join(format!("red_error_{}.log", timestamp));
            let mut file = match File::create(&log_file) {
                Ok(file) => file,
                Err(_) => return,
            };
            let _ = writeln!(file, "Red Editor Error Log");
            let _ = writeln!(file, "Timestamp: {}", timestamp);
            let _ = writeln!(file, "Error: {}", error);
            let _ = writeln!(file, "\nEditor State:");
            let _ = writeln!(file, "File: {:?}", self.filename);
            let _ = writeln!(file, "Modified: {}", self.modified);
            let _ = writeln!(file, "Cursor: {:?}", self.cursor_position);
            let _ = writeln!(file, "Popup: {:?}", self.popup_state);
            let _ = writeln!(file, "Mode: {:?}", self.mode);
            let _ = writeln!(file, "\nLast few lines of content:");
            let start = self.content.len().saturating_sub(5);
            for (i, line) in self.content[start..].iter().enumerate() {
                let _ = writeln!(file, "{}: {}", start + i, line);
            }
        }
    }
    fn check_file_changes(&mut self) -> std::io::Result<()> {
        if self.popup_state != PopupType::None {
            return Ok(());
        }
        if let Some(path) = &self.filename {
            if self.last_file_check.elapsed() < Duration::from_secs(1) {
                return Ok(());
            }
            self.last_file_check = Instant::now();
            if let Ok(metadata) = fs::metadata(path) {
                if let Ok(modified) = metadata.modified() {
                    if modified > self.last_modified.unwrap_or(SystemTime::now())
                        && modified != self.last_modified.unwrap_or(SystemTime::now())
                        && self.last_save_time.map_or(true, |last_save| modified != last_save) {
                        self.popup_state = PopupType::FileChanged;
                        return Ok(());
                    }
                }
            }
        }
        Ok(())
    }
    fn reload_file(&mut self) -> std::io::Result<()> {
        if let Some(path) = &self.filename {
            let content = fs::read_to_string(path)?;
            self.content = content.lines().map(String::from).collect();
            if self.content.is_empty() {
                self.content.push(String::new());
            }
            if let Ok(metadata) = fs::metadata(path) {
                self.last_modified = metadata.modified().ok();
            }
            self.modified = false;
            self.set_status_message("File reloaded from disk");
        }
        Ok(())
    }
    fn handle_suggestion_keys(&mut self, key: KeyEvent) -> bool {
        if !self.showing_suggestions {
            return false;
        }
        match (key.code, key.modifiers) {
            (KeyCode::Tab, KeyModifiers::ALT) => {
                if !self.suggestions.is_empty() {
                    self.suggestion_index = (self.suggestion_index + 1) % self.suggestions.len();
                }
                true
            }
            (KeyCode::Tab, KeyModifiers::NONE) => {
                if !self.suggestions.is_empty() {
                    self.apply_suggestion();
                }
                true
            }
            _ => false
        }
    }
    fn handle_tool_menu_selection(&mut self, selection: usize) {
        match selection {
            0 => self.delete_comments(),
            1 => self.remove_empty_lines(),
            2 => {
                if let Err(e) = self.clear_cache() {
                    self.set_status_message(format!("Error clearing cache: {}", e));
                }
            }
            _ => {}
        }
    }
    fn delete_comments(&mut self) {
        let mut new_content = Vec::new();
        let mut new_cursor_position = self.cursor_position;
        for (line_index, line) in self.content.iter().enumerate() {
            let mut result = line.to_string();
            for comment_start in ["//"] {
                if let Some(pos) = result.find(comment_start) {
                    let before_comment = &result[..pos];
                    let mut in_string = false;
                    let mut escaped = false;
                    for c in before_comment.chars() {
                        if c == '\\' {
                            escaped = !escaped;
                        } else if c == '"' && !escaped {
                            in_string = !in_string;
                            escaped = false;
                        } else {
                            escaped = false;
                        }
                    }
                    if !in_string {
                        result = result[..pos].trim_end().to_string();
                        if line_index == self.cursor_position.1 && self.cursor_position.0 > pos {
                            new_cursor_position.0 = pos;
                        }
                    }
                }
            }
            while let Some(start) = result.find("/*") {
                if let Some(end) = result[start..].find("*/") {
                    let before = &result[..start];
                    let after = &result[start + end + 2..];
                    result = format!("{}{}", before.trim_end(), after);
                    if line_index == self.cursor_position.1 && self.cursor_position.0 > start {
                        new_cursor_position.0 = start;
                    }
                } else {
                    break;
                }
            }
            new_content.push(result);
        }
        self.content = new_content;
        self.cursor_position = new_cursor_position;
        self.modified = true;
        self.set_status_message("Comments deleted");
    }
    fn draw_tool_menu(&mut self, frame: &mut Frame) {
        let area = Rect::new(
            frame.size().width / 3,
            frame.size().height / 3,
            frame.size().width / 3,
            frame.size().height / 3,
        );
        frame.render_widget(Clear, area);
        let popup_block = Block::default()
            .title(" Tools ")
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));
        let inner_area = popup_block.inner(area);
        frame.render_widget(popup_block, area);
        let tools = vec![
            ("  ", "Delete Comments", "Remove all comments from file"),
            ("  ", "Clear Cache", "Clear editor history and cache files"),
        ];
        let text: Vec<Line> = tools.iter().enumerate().map(|(i, (icon, name, desc))| {
            let style = if i == self.tool_menu_selection {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            Line::from(vec![
                Span::raw(format!(" {} ", icon)),
                Span::styled(*name, style),
                Span::raw(" - "),
                Span::styled(*desc, Style::default().fg(Color::Gray))
            ])
        }).collect();
        let paragraph = Paragraph::new(text)
            .alignment(Alignment::Left);
        frame.render_widget(paragraph, inner_area);
    }
    fn replace_all(&mut self) {
        for line in &mut self.content {
            *line = line.replace(&self.search_query, &self.replace_text);
        }
        self.modified = true;
        self.set_status_message("Replacement completed.");
    }
    fn replace_current(&mut self) {
        if let Some((line_index, col_index)) = self.highlighted_matches.get(self.current_match_index) {
            let line = &mut self.content[*line_index];
            line.replace_range(*col_index..*col_index + self.search_query.len(), &self.replace_text);
            self.set_status_message(format!("Replaced occurrence at line {}.", line_index + 1));
        }
    }
    fn save_state(&mut self) {
        let now = Instant::now();
        let current_file = self.filename.clone();
        let current_line = self.cursor_position.1;
        let old_line = self.last_save_state.as_ref()
            .and_then(|state| state.get(current_line))
            .cloned()
            .unwrap_or_default();
        let new_line = self.content.get(current_line)
            .cloned()
            .unwrap_or_default();
        if old_line != new_line {
            let delta = MultiLineDelta {
                start_line: current_line,
                old_lines: vec![old_line],
                new_lines: vec![new_line],
                cursor_before: self.cursor_position,
                cursor_after: self.cursor_position,
                timestamp: now,
                file_id: current_file.clone(),
            };
            self.undo_stack.push((self.content.clone(), self.cursor_position));
            while self.undo_stack.len() > 10000 {
                self.undo_stack.remove(0);
            }
            self.redo_stack.retain(|(state, _)| state != &self.content);
            self.last_save_state = Some(self.content.clone());
            self.last_edit_time = now;
        }
    }
    fn create_new_file(&mut self) -> std::io::Result<()> {
        if !self.temp_filename.is_empty() {
            let path = self.current_dir.join(&self.temp_filename);
            if path.exists() {
                self.set_status_message("File already exists");
                return Ok(());
            }
            fs::write(&path, "")?;
            self.file_entries = Self::read_directory(&self.current_dir)?;
            if let Some(index) = self.file_entries.iter().position(|entry| entry.path == path) {
                self.file_explorer_selection = index;
            }
            self.set_status_message(format!("Created file: {}", self.temp_filename));
            self.popup_state = PopupType::None;
            self.temp_filename.clear();
        }
        Ok(())
    }
    fn create_new_directory(&mut self) -> std::io::Result<()> {
        if !self.temp_filename.is_empty() {
            let path = self.current_dir.join(&self.temp_filename);
            if path.exists() {
                self.set_status_message("Directory already exists");
                return Ok(());
            }
            fs::create_dir(&path)?;
            self.file_entries = Self::read_directory(&self.current_dir)?;
            if let Some(index) = self.file_entries.iter().position(|entry| entry.path == path) {
                self.file_explorer_selection = index;
            }
            self.set_status_message(format!("Created directory: {}", self.temp_filename));
            self.popup_state = PopupType::None;
            self.temp_filename.clear();
        }
        Ok(())
    }
    fn clear_cache(&mut self) -> std::io::Result<()> {
        self.recent_files.clear();
        if let Some(home) = env::var("HOME").ok().map(PathBuf::from) {
            let config_dir = home.join(".config").join("red");
            let history_file = config_dir.join("history");
            if history_file.exists() {
                fs::remove_file(history_file)?;
            }
            let logs_dir = config_dir.join("logs");
            if logs_dir.exists() {
                fs::remove_dir_all(&logs_dir)?;
                fs::create_dir(&logs_dir)?;
            }
        }
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.last_save_state = None;
        self.set_status_message("Cache cleared");
        Ok(())
    }
    fn remove_empty_lines(&mut self) {
        self.content.retain(|line| !line.trim().is_empty());
        self.modified = true;
        self.set_status_message("Empty lines removed");
    }
}
impl Drop for Editor {
    fn drop(&mut self) {
        if let Err(e) = crossterm::execute!(
            self.terminal.backend_mut(),
            terminal::LeaveAlternateScreen
        ) {
            eprintln!("Failed to leave alternate screen: {}", e);
        }
        if let Err(e) = terminal::disable_raw_mode() {
            eprintln!("Failed to disable raw mode: {}", e);
        }
    }
}
fn main() {
    match Editor::new() {
        Ok(mut editor) => {
            if let Err(e) = editor.run() {
                let _ = editor.cleanup();
                eprintln!("\nEditor error: {}", e);
                std::process::exit(1);
            }
        }
        Err(e) => {
            let _ = terminal::disable_raw_mode();
            let _ = crossterm::execute!(
                std::io::stdout(),
                terminal::LeaveAlternateScreen
            );
            match e.kind() {
                std::io::ErrorKind::PermissionDenied => {
                    eprintln!("\nPermission denied. Use 'sudo red' to edit this file.\n");
                }
                std::io::ErrorKind::NotFound => {
                    eprintln!("\nFile not found. A new file will be created on save.\n");
                }
                std::io::ErrorKind::IsADirectory => {
                    eprintln!("\nCannot edit a directory.\n");
                }
                _ => {
                    eprintln!("\nError: {}\n", e);
                }
            }
            std::process::exit(1);
        }
    }
}
