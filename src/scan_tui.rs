use anyhow::Result;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};
use std::io::Stdout;
use std::time::Instant;

use crate::tui;

/// Live scan progress state, rendered to the terminal.
pub struct ScanProgress {
    current_dir: String,
    files_scanned: u64,
    files_hashed: u64,
    files_deleted: u64,
    log_lines: Vec<String>,
    start: Instant,
    /// None when not in a TTY — falls back to plain stdout output.
    terminal: Option<Terminal<CrosstermBackend<Stdout>>>,
}

impl ScanProgress {
    pub fn new() -> Self {
        Self {
            current_dir: String::new(),
            files_scanned: 0,
            files_hashed: 0,
            files_deleted: 0,
            log_lines: Vec::new(),
            start: Instant::now(),
            terminal: None,
        }
    }

    pub fn start(&mut self) -> Result<()> {
        self.start = Instant::now();
        match tui::enter() {
            Ok(t) => {
                self.terminal = Some(t);
                self.render()?;
            }
            Err(_) => {
                // Not a TTY (e.g. piped output, VS Code embedded terminal) — plain mode.
                eprintln!("(scan progress: plain output mode)");
            }
        }
        Ok(())
    }

    pub fn set_current_dir(&mut self, dir: String) {
        self.current_dir = dir;
        if self.terminal.is_some() {
            let _ = self.render();
        } else {
            eprintln!("Scanning: {}", self.current_dir);
        }
    }

    pub fn inc_scanned(&mut self) {
        self.files_scanned += 1;
        let _ = self.render();
    }

    pub fn inc_hashed(&mut self) {
        self.files_hashed += 1;
        let _ = self.render();
    }

    pub fn inc_deleted(&mut self) {
        self.files_deleted += 1;
        let _ = self.render();
    }

    pub fn log(&mut self, msg: String) {
        if self.terminal.is_none() {
            eprintln!("{}", msg);
        }
        self.log_lines.push(msg);
        if self.log_lines.len() > 100 {
            self.log_lines.remove(0);
        }
        let _ = self.render();
    }

    pub fn finish(mut self, duplicate_groups: usize) -> Result<()> {
        if let Some(ref mut t) = self.terminal {
            tui::leave(t)?;
        }
        let elapsed = self.start.elapsed();
        println!(
            "Scan complete in {:.1}s — {} files scanned, {} hashed, {} deleted, {} duplicate groups",
            elapsed.as_secs_f64(),
            self.files_scanned,
            self.files_hashed,
            self.files_deleted,
            duplicate_groups,
        );
        Ok(())
    }

    fn render(&mut self) -> Result<()> {
        let Some(ref mut terminal) = self.terminal else {
            return Ok(());
        };

        let current_dir = self.current_dir.clone();
        let files_scanned = self.files_scanned;
        let files_hashed = self.files_hashed;
        let files_deleted = self.files_deleted;
        let elapsed = self.start.elapsed();
        let log_lines = self.log_lines.clone();

        terminal.draw(|f| {
            let area = f.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(7), Constraint::Min(0)])
                .split(area);

            // Status panel
            let elapsed_str = format!("{:.1}s", elapsed.as_secs_f64());
            // "Scanning: " is 10 chars; subtract 2 for borders.
            let path_width = (chunks[0].width as usize).saturating_sub(2 + 10);
            let truncated_dir = tui::truncate_path(&current_dir, path_width);
            let status_text = vec![
                Line::from(vec![
                    Span::styled("Scanning: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(truncated_dir),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("Files scanned:  ", Style::default().fg(Color::Cyan)),
                    Span::raw(files_scanned.to_string()),
                ]),
                Line::from(vec![
                    Span::styled("Files hashed:   ", Style::default().fg(Color::Yellow)),
                    Span::raw(files_hashed.to_string()),
                ]),
                Line::from(vec![
                    Span::styled("Files deleted:  ", Style::default().fg(Color::Red)),
                    Span::raw(files_deleted.to_string()),
                ]),
                Line::from(vec![
                    Span::styled("Elapsed:        ", Style::default().fg(Color::Green)),
                    Span::raw(elapsed_str),
                ]),
            ];

            let status = Paragraph::new(status_text)
                .block(Block::default().borders(Borders::ALL).title(" fdedupe — scanning "))
                .wrap(Wrap { trim: false });
            f.render_widget(status, chunks[0]);

            // Log panel
            let log_text: Vec<Line> = log_lines.iter().map(|l| Line::from(l.as_str())).collect();
            let log = Paragraph::new(log_text)
                .block(Block::default().borders(Borders::ALL).title(" Log "))
                .wrap(Wrap { trim: true });
            f.render_widget(log, chunks[1]);
        })?;

        Ok(())
    }
}
