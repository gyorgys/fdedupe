use anyhow::Result;
use crossterm::event::KeyCode;
use globset::{Glob, GlobSetBuilder};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use std::time::Duration;

use crate::cli::RemoveArgs;
use crate::config::Config;
use crate::db::{Db, DuplicateGroup, FileRow};
use crate::tui::{self, fmt_size};

pub fn run(args: &RemoveArgs, _config: &Config, db: &Db) -> Result<()> {
    let mut groups = db.duplicate_groups()?;
    if groups.is_empty() {
        println!("No duplicates found. Run 'fdedupe scan' first.");
        return Ok(());
    }

    let rules = db.all_rules()?;
    let mut terminal = tui::enter()?;
    let result = run_loop(&mut terminal, &mut groups, &rules, args.dry_run, db);
    tui::leave(&mut terminal)?;
    result
}

// ── Per-group action ──────────────────────────────────────────────────────────

#[derive(Clone, PartialEq)]
enum FileAction {
    Keep,
    Delete,
    Undecided,
}

struct GroupState {
    files: Vec<FileRow>,
    actions: Vec<FileAction>,
    list_state: ListState,
    input_mode: Option<InputMode>,
    rule_pattern: String,
    rule_priority: String,
    status_msg: String,
}

#[derive(Clone)]
enum InputMode {
    RulePattern,
    RulePriority,
}

impl GroupState {
    fn new(group: &DuplicateGroup) -> Self {
        let n = group.files.len();
        let mut ls = ListState::default();
        ls.select(Some(0));
        Self {
            files: group.files.clone(),
            actions: vec![FileAction::Undecided; n],
            list_state: ls,
            input_mode: None,
            rule_pattern: String::new(),
            rule_priority: String::new(),
            status_msg: String::new(),
        }
    }

    fn apply_rules(&mut self, rules: &[crate::db::RuleRow]) {
        if rules.is_empty() {
            return;
        }
        // Build globsets for each rule
        let scored: Vec<i64> = self
            .files
            .iter()
            .map(|f| {
                rules
                    .iter()
                    .filter(|r| {
                        Glob::new(&r.pattern)
                            .ok()
                            .and_then(|g| {
                                let mut b = GlobSetBuilder::new();
                                b.add(g);
                                b.build().ok()
                            })
                            .map(|gs| gs.is_match(&f.canonical_path))
                            .unwrap_or(false)
                    })
                    .map(|r| r.priority)
                    .max()
                    .unwrap_or(i64::MIN)
            })
            .collect();

        // If there's a unique maximum, auto-decide
        let max_score = scored.iter().copied().max().unwrap_or(i64::MIN);
        let max_count = scored.iter().filter(|&&s| s == max_score).count();
        if max_count == 1 {
            for (i, &score) in scored.iter().enumerate() {
                if score == max_score {
                    self.actions[i] = FileAction::Keep;
                } else {
                    self.actions[i] = FileAction::Delete;
                }
            }
            self.status_msg = "Auto-resolved by priority rule.".into();
        }
    }

    fn is_decided(&self) -> bool {
        self.actions.iter().any(|a| *a == FileAction::Keep)
            && self.actions.iter().any(|a| *a == FileAction::Delete)
    }

    fn move_selection(&mut self, delta: i32) {
        let len = self.files.len() as i32;
        let cur = self.list_state.selected().unwrap_or(0) as i32;
        self.list_state
            .select(Some((cur + delta).clamp(0, len - 1) as usize));
    }

    fn mark_delete(&mut self) {
        if let Some(idx) = self.list_state.selected() {
            self.actions[idx] = FileAction::Delete;
            // All others → keep
            for (i, a) in self.actions.iter_mut().enumerate() {
                if i != idx {
                    *a = FileAction::Keep;
                }
            }
        }
    }

    fn mark_keep(&mut self) {
        if let Some(idx) = self.list_state.selected() {
            self.actions[idx] = FileAction::Keep;
            // All others → delete
            for (i, a) in self.actions.iter_mut().enumerate() {
                if i != idx {
                    *a = FileAction::Delete;
                }
            }
        }
    }
}

// ── Main loop ─────────────────────────────────────────────────────────────────

fn run_loop(
    terminal: &mut tui::Term,
    groups: &mut Vec<DuplicateGroup>,
    initial_rules: &[crate::db::RuleRow],
    dry_run: bool,
    db: &Db,
) -> Result<()> {
    let total = groups.len();
    let mut idx = 0;
    let mut current_rules: Vec<crate::db::RuleRow> = initial_rules.to_vec();

    while idx < groups.len() {
        let group = &groups[idx];
        let mut gs = GroupState::new(group);
        gs.apply_rules(&current_rules);

        let result = group_loop(terminal, &mut gs, idx, total, dry_run, db, &mut current_rules)?;

        match result {
            GroupResult::Confirm => {
                let files_to_delete: Vec<String> = gs
                    .files
                    .iter()
                    .zip(gs.actions.iter())
                    .filter(|(_, a)| **a == FileAction::Delete)
                    .map(|(f, _)| f.canonical_path.clone())
                    .collect();

                if !dry_run {
                    for path in &files_to_delete {
                        if let Err(e) = std::fs::remove_file(path) {
                            eprintln!("Failed to delete {}: {}", path, e);
                        } else {
                            db.delete_file_by_path(path)?;
                        }
                    }
                }
                idx += 1;
            }
            GroupResult::Skip => {
                idx += 1;
            }
            GroupResult::Quit => break,
        }
    }

    Ok(())
}

enum GroupResult {
    Confirm,
    Skip,
    Quit,
}

fn group_loop(
    terminal: &mut tui::Term,
    gs: &mut GroupState,
    group_idx: usize,
    total: usize,
    dry_run: bool,
    db: &Db,
    rules: &mut Vec<crate::db::RuleRow>,
) -> Result<GroupResult> {
    loop {
        let size_each = gs.files.first().map(|f| f.size).unwrap_or(0);

        terminal.draw(|f| {
            let area = f.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(0),
                    Constraint::Length(if gs.input_mode.is_some() { 4 } else { 3 }),
                ])
                .split(area);

            // Header
            let dry_tag = if dry_run { " [DRY RUN]" } else { "" };
            let title = format!(
                " fdedupe — remove{}  (group {} of {}, {} each) ",
                dry_tag,
                group_idx + 1,
                total,
                fmt_size(size_each)
            );
            let header = Paragraph::new(Line::from(gs.status_msg.as_str()))
                .block(Block::default().borders(Borders::ALL).title(title));
            f.render_widget(header, chunks[0]);

            // File list
            let items: Vec<ListItem> = gs
                .files
                .iter()
                .zip(gs.actions.iter())
                .map(|(file, action)| {
                    let (marker, style) = match action {
                        FileAction::Keep => (
                            "[KEEP]   ",
                            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                        ),
                        FileAction::Delete => (
                            "[DELETE] ",
                            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                        ),
                        FileAction::Undecided => ("[?]      ", Style::default()),
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(marker, style),
                        Span::raw(&file.canonical_path),
                    ]))
                })
                .collect();

            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title(" Files "))
                .highlight_style(Style::default().bg(Color::DarkGray));
            f.render_stateful_widget(list, chunks[1], &mut gs.list_state);

            // Footer / input
            if let Some(ref mode) = gs.input_mode {
                let (prompt, value) = match mode {
                    InputMode::RulePattern => ("Glob pattern: ", gs.rule_pattern.as_str()),
                    InputMode::RulePriority => ("Priority (integer): ", gs.rule_priority.as_str()),
                };
                let input_text = vec![
                    Line::from(Span::raw(format!("{}{}_", prompt, value))),
                    Line::from(Span::styled(
                        "  Enter to confirm   Esc to cancel",
                        Style::default().fg(Color::DarkGray),
                    )),
                ];
                let input = Paragraph::new(input_text)
                    .block(Block::default().borders(Borders::ALL).title(" Add Rule "));
                f.render_widget(input, chunks[2]);
            } else {
                let footer = Paragraph::new(Line::from(
                    "  ↑↓ select   k keep   d/Enter delete   r add rule   s skip   q quit",
                ))
                .style(Style::default().fg(Color::DarkGray));
                f.render_widget(footer, chunks[2]);
            }
        })?;

        if let Some(key) = tui::next_key(Duration::from_millis(100))? {
            // Input mode handling
            if let Some(ref mode) = gs.input_mode.clone() {
                match key.code {
                    KeyCode::Esc => {
                        gs.input_mode = None;
                        gs.rule_pattern.clear();
                        gs.rule_priority.clear();
                    }
                    KeyCode::Enter => match mode {
                        InputMode::RulePattern => {
                            if !gs.rule_pattern.is_empty() {
                                gs.input_mode = Some(InputMode::RulePriority);
                            }
                        }
                        InputMode::RulePriority => {
                            let priority: i64 = gs.rule_priority.parse().unwrap_or(0);
                            db.insert_rule(&gs.rule_pattern, priority)?;
                            let new_rule = crate::db::RuleRow {
                                id: 0,
                                pattern: gs.rule_pattern.clone(),
                                priority,
                            };
                            rules.push(new_rule);
                            gs.status_msg =
                                format!("Rule added: {} (priority {})", gs.rule_pattern, priority);
                            gs.input_mode = None;
                            gs.rule_pattern.clear();
                            gs.rule_priority.clear();
                            gs.apply_rules(rules);
                        }
                    },
                    KeyCode::Backspace => {
                        match mode {
                            InputMode::RulePattern => {
                                gs.rule_pattern.pop();
                            }
                            InputMode::RulePriority => {
                                gs.rule_priority.pop();
                            }
                        }
                    }
                    KeyCode::Char(c) => match mode {
                        InputMode::RulePattern => gs.rule_pattern.push(c),
                        InputMode::RulePriority => {
                            if c.is_ascii_digit() || (c == '-' && gs.rule_priority.is_empty()) {
                                gs.rule_priority.push(c);
                            }
                        }
                    },
                    _ => {}
                }
                continue;
            }

            // Normal mode
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(GroupResult::Quit),
                KeyCode::Char('s') => return Ok(GroupResult::Skip),
                KeyCode::Up => gs.move_selection(-1),
                KeyCode::Down => gs.move_selection(1),
                KeyCode::Char('k') => gs.mark_keep(),
                KeyCode::Char('d') | KeyCode::Enter => {
                    if gs.input_mode.is_none() {
                        if key.code == KeyCode::Enter && gs.is_decided() {
                            return Ok(GroupResult::Confirm);
                        }
                        gs.mark_delete();
                    }
                }
                KeyCode::Char('r') => {
                    gs.input_mode = Some(InputMode::RulePattern);
                }
                KeyCode::Char(' ') if gs.is_decided() => return Ok(GroupResult::Confirm),
                _ => {}
            }
        }
    }
}
