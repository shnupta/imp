//! TUI for managing multiple imp agent sessions.

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Tabs},
};
use std::io::{stdout, Stdout};

use crate::db::Database;
use crate::tmux;

/// Session info for display
struct SessionInfo {
    id: String,
    project: Option<String>,
    preview: String,
    pane: Option<String>,
    pid: Option<u32>,
    is_active: bool,
    created_at: String,
}

/// App state
struct App {
    sessions: Vec<SessionInfo>,
    active_tab: usize, // 0 = Active, 1 = History
    list_state: ListState,
    should_quit: bool,
    jump_to_pane: Option<String>,
}

impl App {
    fn new() -> Result<Self> {
        let mut app = App {
            sessions: vec![],
            active_tab: 0,
            list_state: ListState::default(),
            should_quit: false,
            jump_to_pane: None,
        };
        app.refresh_sessions()?;
        if !app.filtered_sessions().is_empty() {
            app.list_state.select(Some(0));
        }
        Ok(app)
    }

    fn refresh_sessions(&mut self) -> Result<()> {
        let db = Database::open()?;
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        
        // Get today's sessions from DB
        let db_sessions = db.list_sessions_for_date(&today)?;
        
        // Get registered panes
        let panes = tmux::list_registered_panes();
        
        self.sessions = db_sessions
            .into_iter()
            .map(|(id, project, created_at)| {
                // Find pane info for this session
                let pane_info = panes.iter().find(|p| p.session_id == id);
                let (pid, pane, is_active) = match pane_info {
                    Some(info) => {
                        let alive = tmux::is_process_alive(info.pid);
                        (Some(info.pid), Some(info.pane.clone()), alive)
                    }
                    None => (None, None, false),
                };
                
                // Get first user message as preview
                let preview = db.get_first_user_message(&id)
                    .unwrap_or_default()
                    .unwrap_or_else(|| "(no messages)".to_string());
                let preview = preview.chars().take(60).collect::<String>();
                let preview = if preview.len() >= 60 {
                    format!("{}...", preview)
                } else {
                    preview
                };
                
                SessionInfo {
                    id,
                    project,
                    preview,
                    pane,
                    pid,
                    is_active,
                    created_at,
                }
            })
            .collect();
        
        Ok(())
    }

    fn filtered_sessions(&self) -> Vec<&SessionInfo> {
        self.sessions
            .iter()
            .filter(|s| {
                if self.active_tab == 0 {
                    s.is_active
                } else {
                    !s.is_active
                }
            })
            .collect()
    }

    fn next_tab(&mut self) {
        self.active_tab = (self.active_tab + 1) % 2;
        self.list_state.select(Some(0));
    }

    fn next_item(&mut self) {
        let items = self.filtered_sessions();
        if items.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => (i + 1) % items.len(),
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn prev_item(&mut self) {
        let items = self.filtered_sessions();
        if items.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    items.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn select_current(&mut self) {
        let sessions = self.filtered_sessions();
        if let Some(i) = self.list_state.selected() {
            if let Some(session) = sessions.get(i) {
                if let Some(ref pane) = session.pane {
                    if session.is_active {
                        self.jump_to_pane = Some(pane.clone());
                        self.should_quit = true;
                    }
                }
            }
        }
    }
}

fn ui(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Tabs
            Constraint::Min(0),    // List
            Constraint::Length(3), // Help
        ])
        .split(frame.area());

    // Tabs
    let tabs = Tabs::new(vec!["Active", "History"])
        .block(Block::default().borders(Borders::ALL).title("Sessions"))
        .select(app.active_tab)
        .highlight_style(Style::default().fg(Color::Yellow).bold());
    frame.render_widget(tabs, chunks[0]);

    // Session list
    let sessions = app.filtered_sessions();
    let items: Vec<ListItem> = sessions
        .iter()
        .map(|s| {
            let project = s.project.as_deref().unwrap_or("(no project)");
            let status = if s.is_active { "●" } else { "○" };
            let pane = s.pane.as_deref().unwrap_or("-");
            let line = format!(
                "{} {} [{}] {} | {}",
                status, s.created_at, pane, project, s.preview
            );
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL))
        .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::White))
        .highlight_symbol("▶ ");
    frame.render_stateful_widget(list, chunks[1], &mut app.list_state);

    // Help
    let help = Paragraph::new("Tab: switch tabs | ↑↓/jk: navigate | Enter: jump to pane | r: refresh | q: quit")
        .block(Block::default().borders(Borders::ALL));
    frame.render_widget(help, chunks[2]);
}

fn run_tui(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|f| ui(f, app))?;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        app.should_quit = true;
                    }
                    KeyCode::Tab => app.next_tab(),
                    KeyCode::Down | KeyCode::Char('j') => app.next_item(),
                    KeyCode::Up | KeyCode::Char('k') => app.prev_item(),
                    KeyCode::Enter => app.select_current(),
                    KeyCode::Char('r') => {
                        let _ = app.refresh_sessions();
                    }
                    _ => {}
                }
            }
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}

pub fn run() -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    // Create app and run
    let mut app = App::new()?;
    let result = run_tui(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    result?;

    // Jump to pane if requested
    if let Some(pane) = app.jump_to_pane {
        tmux::switch_to_pane(&pane)?;
    }

    Ok(())
}
