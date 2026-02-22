use anyhow::Result;
use crossterm::event::KeyCode;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::db::{Db, FileRow};
use crate::tui::{self, fmt_size};

// ── Entry types shown in the list ────────────────────────────────────────────

#[derive(Clone)]
enum Entry {
    Subdir { path: String, dup_count: i64, dup_size: i64 },
    File { row: FileRow, is_dup: bool },
}

// ── State ─────────────────────────────────────────────────────────────────────

struct State<'a> {
    db: &'a Db,
    root: PathBuf,
    current: PathBuf,
    entries: Vec<Entry>,
    list_state: ListState,
    dup_count: i64,
    dup_size: i64,
}

impl<'a> State<'a> {
    fn new(root: PathBuf, db: &'a Db) -> Result<Self> {
        let current = root.clone();
        let mut s = Self {
            db,
            root,
            current: current.clone(),
            entries: Vec::new(),
            list_state: ListState::default(),
            dup_count: 0,
            dup_size: 0,
        };
        s.load_dir(&current.clone())?;
        Ok(s)
    }

    fn load_dir(&mut self, dir: &Path) -> Result<()> {
        self.current = dir.to_path_buf();
        self.entries.clear();
        self.list_state.select(None);

        let dir_str = dir.to_string_lossy();
        let (count, size) = self.db.duplicate_stats_under(&dir_str)?;
        self.dup_count = count;
        self.dup_size = size;

        let dir_row = self.db.get_directory(&dir_str)?;

        // Subdirectories
        let children = self.db.child_directories(&dir_str)?;
        for child in children {
            let (dc, ds) = self.db.duplicate_stats_under(&child.canonical_path)?;
            self.entries.push(Entry::Subdir {
                path: child.canonical_path,
                dup_count: dc,
                dup_size: ds,
            });
        }

        // Files
        if let Some(row) = dir_row {
            let dup_files = self.db.duplicate_files_in_dir(row.id)?;
            let dup_paths: std::collections::HashSet<i64> =
                dup_files.iter().map(|f| f.id).collect();
            let all_files = self.db.files_in_directory(row.id)?;
            for f in all_files {
                let is_dup = dup_paths.contains(&f.id);
                self.entries.push(Entry::File { row: f, is_dup });
            }
        }

        if !self.entries.is_empty() {
            self.list_state.select(Some(0));
        }

        Ok(())
    }

    fn navigate_into(&mut self) -> Result<()> {
        if let Some(idx) = self.list_state.selected() {
            if let Some(Entry::Subdir { path, .. }) = self.entries.get(idx).cloned() {
                self.load_dir(&PathBuf::from(path))?;
            }
        }
        Ok(())
    }

    fn navigate_up(&mut self) -> Result<()> {
        if self.current == self.root {
            return Ok(());
        }
        if let Some(parent) = self.current.parent() {
            let parent = parent.to_path_buf();
            self.load_dir(&parent)?;
        }
        Ok(())
    }

    fn move_selection(&mut self, delta: i32) {
        if self.entries.is_empty() {
            return;
        }
        let len = self.entries.len() as i32;
        let current = self.list_state.selected().unwrap_or(0) as i32;
        let next = (current + delta).clamp(0, len - 1) as usize;
        self.list_state.select(Some(next));
    }

    fn page_size(&self) -> i32 {
        20
    }
}

// ── Public entry point ────────────────────────────────────────────────────────

pub fn run(root: &Path, db: &Db) -> Result<()> {
    let mut terminal = tui::enter()?;
    let result = run_loop(&mut terminal, root, db);
    tui::leave(&mut terminal)?;
    result
}

fn run_loop(terminal: &mut tui::Term, root: &Path, db: &Db) -> Result<()> {
    let mut state = State::new(root.to_path_buf(), db)?;

    loop {
        // Snapshot data needed by the draw closure (avoids borrow issues)
        let current_str = state.current.to_string_lossy().into_owned();
        let dup_count = state.dup_count;
        let dup_size = state.dup_size;

        terminal.draw(|f| {
            let area = f.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(2)])
                .split(area);

            // Header
            let header_text = vec![Line::from(vec![
                Span::styled(&current_str, Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("  —  "),
                Span::styled(
                    format!("{} duplicates, {}", dup_count, fmt_size(dup_size)),
                    Style::default().fg(if dup_count > 0 { Color::Yellow } else { Color::Green }),
                ),
            ])];
            let header = Paragraph::new(header_text)
                .block(Block::default().borders(Borders::ALL).title(" fdedupe — list "));
            f.render_widget(header, chunks[0]);

            // Entry list
            let items: Vec<ListItem> = state.entries.iter().map(|e| match e {
                Entry::Subdir { path, dup_count, dup_size } => {
                    let name = path.rsplit('/').next().unwrap_or(path.as_str());
                    let style = if *dup_count > 0 {
                        Style::default().fg(Color::Yellow)
                    } else {
                        Style::default()
                    };
                    let label = if *dup_count > 0 {
                        format!("  {}/   ({} dups, {})", name, dup_count, fmt_size(*dup_size))
                    } else {
                        format!("  {}/", name)
                    };
                    ListItem::new(label).style(style)
                }
                Entry::File { row, is_dup } => {
                    let style = if *is_dup {
                        Style::default().fg(Color::Red)
                    } else {
                        Style::default()
                    };
                    let label = format!("    {}   ({})", row.name, fmt_size(row.size));
                    ListItem::new(label).style(style)
                }
            }).collect();

            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL))
                .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD));
            f.render_stateful_widget(list, chunks[1], &mut state.list_state);

            // Footer
            let footer = Paragraph::new(Line::from(
                "  ↑↓ navigate   → / Enter / Space: open dir   ← / Backspace: up   q / Esc: quit",
            ))
            .style(Style::default().fg(Color::DarkGray));
            f.render_widget(footer, chunks[2]);
        })?;

        // Input
        if let Some(key) = tui::next_key(Duration::from_millis(200))? {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Up => state.move_selection(-1),
                KeyCode::Down => state.move_selection(1),
                KeyCode::PageUp => {
                    let ps = state.page_size();
                    state.move_selection(-ps);
                }
                KeyCode::PageDown => {
                    let ps = state.page_size();
                    state.move_selection(ps);
                }
                KeyCode::Right | KeyCode::Enter | KeyCode::Char(' ') => {
                    state.navigate_into()?;
                }
                KeyCode::Left | KeyCode::Backspace => {
                    state.navigate_up()?;
                }
                _ => {}
            }
        }
    }

    Ok(())
}
