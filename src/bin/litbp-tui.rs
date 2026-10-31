//! TUI shell for litbp, built on ratatui + crossterm.

use std::collections::VecDeque;
use std::io;
use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use litbp::{img_to_snbt, validate_grid, Options, Resampling};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph};
use ratatui::{Frame, Terminal};

/// Posterize steps offered by the Quant selector (0 = off).
const QUANT_STEPS: [u8; 6] = [0, 4, 8, 16, 32, 64];

#[derive(Copy, Clone, PartialEq, Eq)]
enum Focus {
    Input,
    Output,
    Scale,
    Grid,
    Block,
    Filter,
    Merge,
    Quant,
    Run,
    Copy,
}

impl Focus {
    const ALL: [Focus; 10] = [
        Focus::Input,
        Focus::Output,
        Focus::Scale,
        Focus::Grid,
        Focus::Block,
        Focus::Filter,
        Focus::Merge,
        Focus::Quant,
        Focus::Run,
        Focus::Copy,
    ];

    fn next(self) -> Self {
        let i = Self::ALL.iter().position(|&f| f == self).unwrap();
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    fn prev(self) -> Self {
        let i = Self::ALL.iter().position(|&f| f == self).unwrap();
        Self::ALL[(i + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

enum Msg {
    Progress(u64, u64),
    Done(Result<String, String>),
}

/// Trim whitespace and one pair of surrounding double quotes, so paths
/// pasted from Explorer's "Copy as path" work directly.
fn unquote(s: &str) -> &str {
    let t = s.trim();
    t.strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .unwrap_or(t)
}

struct App {
    input: String,
    output: String,
    scale: String,
    grid: String,
    block: String,
    filter: usize,
    merge: bool,
    quant_idx: usize,
    focus: Focus,
    editing: bool,
    input_info: Option<Result<(u32, u32), String>>,
    log: VecDeque<(String, Color)>,
    progress: Option<(u64, u64)>,
    running: bool,
    rx: Option<Receiver<Msg>>,
    quit: bool,
}

impl App {
    fn new() -> Self {
        let mut app = Self {
            input: String::new(),
            output: String::new(),
            scale: "1.0".to_string(),
            grid: "16".to_string(),
            block: "littletiles:ltcoloredblock".to_string(),
            filter: 0,
            merge: false,
            quant_idx: 0,
            focus: Focus::Input,
            editing: false,
            input_info: None,
            log: VecDeque::new(),
            progress: None,
            running: false,
            rx: None,
            quit: false,
        };
        app.log(
            "Only Input is required. Run converts (and saves if Output is set), Copy converts to clipboard.",
            Color::Gray,
        );
        app
    }

    fn log(&mut self, msg: impl Into<String>, color: Color) {
        self.log.push_back((msg.into(), color));
        while self.log.len() > 200 {
            self.log.pop_front();
        }
    }

    fn text_field(&mut self, focus: Focus) -> Option<&mut String> {
        match focus {
            Focus::Input => Some(&mut self.input),
            Focus::Output => Some(&mut self.output),
            Focus::Scale => Some(&mut self.scale),
            Focus::Grid => Some(&mut self.grid),
            Focus::Block => Some(&mut self.block),
            _ => None,
        }
    }

    fn refresh_input_info(&mut self) {
        self.input_info = if self.input.trim().is_empty() {
            None
        } else {
            Some(image::image_dimensions(unquote(&self.input)).map_err(|e| e.to_string()))
        };
    }

    fn start_run(&mut self, to_clipboard: bool) {
        if self.running {
            return;
        }
        let input = unquote(&self.input).to_string();
        let output = unquote(&self.output).to_string();
        if input.is_empty() {
            self.log("Input image path is empty", Color::Red);
            return;
        }
        if !Path::new(&input).is_file() {
            self.log(format!("Input file not found: {input}"), Color::Red);
            return;
        }
        let scale: f64 = match self.scale.trim().parse() {
            Ok(v) if v > 0.0 => v,
            _ => {
                self.log("Invalid scale (must be a positive number)", Color::Red);
                return;
            }
        };
        let grid: i32 = match self.grid.trim().parse() {
            Ok(v) if validate_grid(v) => v,
            _ => {
                self.log("Invalid grid (1 or a multiple of 4, max 32)", Color::Red);
                return;
            }
        };

        let opts = Options {
            scale,
            grid,
            block: self.block.clone(),
            resampling: Resampling::ALL[self.filter],
            merge: self.merge,
            quantize: QUANT_STEPS[self.quant_idx],
        };
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.running = true;
        self.progress = Some((0, 1));
        self.log(format!("Converting {input} ..."), Color::Gray);

        thread::spawn(move || {
            let txp = tx.clone();
            let result = img_to_snbt(&input, &opts, move |done, cells| {
                let _ = txp.send(Msg::Progress(done, cells));
            });
            let msg = result.and_then(|conv| {
                let stats = format!("{}x{}, {} tiles", conv.width, conv.height, conv.tiles);
                if to_clipboard {
                    let text = conv.snbt.serialize();
                    let len = text.len();
                    let mut clipboard = arboard::Clipboard::new()
                        .map_err(|e| format!("Clipboard unavailable: {e}"))?;
                    clipboard
                        .set_text(text)
                        .map_err(|e| format!("Cannot copy to clipboard: {e}"))?;
                    Ok(format!("Copied {len} chars to clipboard ({stats})"))
                } else if output.is_empty() {
                    Ok(format!("Done ({stats}), no output path given"))
                } else {
                    std::fs::write(&output, conv.snbt.serialize())
                        .map_err(|e| format!("Cannot write {output}: {e}"))?;
                    Ok(format!("Saved {output} ({stats})"))
                }
            });
            let _ = tx.send(Msg::Done(msg));
        });
    }

    fn drain_msgs(&mut self) {
        let mut msgs = Vec::new();
        if let Some(rx) = &self.rx {
            while let Ok(m) = rx.try_recv() {
                msgs.push(m);
            }
        }
        for m in msgs {
            match m {
                Msg::Progress(done, cells) => self.progress = Some((done, cells)),
                Msg::Done(Ok(s)) => {
                    self.running = false;
                    self.progress = None;
                    self.log(s, Color::Green);
                }
                Msg::Done(Err(e)) => {
                    self.running = false;
                    self.progress = None;
                    self.log(e, Color::Red);
                }
            }
        }
        if !self.running {
            self.rx = None;
        }
    }

    fn on_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        if self.editing {
            match key.code {
                KeyCode::Enter | KeyCode::Esc => {
                    self.editing = false;
                    if self.focus == Focus::Input {
                        self.refresh_input_info();
                    }
                }
                KeyCode::Char(c) => {
                    if let Some(f) = self.text_field(self.focus) {
                        f.push(c);
                    }
                }
                KeyCode::Backspace => {
                    if let Some(f) = self.text_field(self.focus) {
                        f.pop();
                    }
                }
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.quit = true
            }
            KeyCode::Up | KeyCode::BackTab => self.focus = self.focus.prev(),
            KeyCode::Down | KeyCode::Tab => self.focus = self.focus.next(),
            KeyCode::Left if self.focus == Focus::Filter => {
                self.filter =
                    (self.filter + Resampling::ALL.len() - 1) % Resampling::ALL.len();
            }
            KeyCode::Right if self.focus == Focus::Filter => {
                self.filter = (self.filter + 1) % Resampling::ALL.len();
            }
            KeyCode::Left | KeyCode::Right if self.focus == Focus::Merge => {
                self.merge = !self.merge;
            }
            KeyCode::Left if self.focus == Focus::Quant => {
                self.quant_idx =
                    (self.quant_idx + QUANT_STEPS.len() - 1) % QUANT_STEPS.len();
            }
            KeyCode::Right if self.focus == Focus::Quant => {
                self.quant_idx = (self.quant_idx + 1) % QUANT_STEPS.len();
            }
            KeyCode::Left if self.focus == Focus::Copy => self.focus = Focus::Run,
            KeyCode::Right if self.focus == Focus::Run => self.focus = Focus::Copy,
            KeyCode::Enter => match self.focus {
                Focus::Filter => self.filter = (self.filter + 1) % Resampling::ALL.len(),
                Focus::Merge => self.merge = !self.merge,
                Focus::Quant => self.quant_idx = (self.quant_idx + 1) % QUANT_STEPS.len(),
                Focus::Run => self.start_run(false),
                Focus::Copy => self.start_run(true),
                _ => self.editing = true,
            },
            _ => {}
        }
    }
}

fn value_of<'a>(app: &'a App, focus: Focus) -> &'a str {
    match focus {
        Focus::Input => &app.input,
        Focus::Output => &app.output,
        Focus::Scale => &app.scale,
        Focus::Grid => &app.grid,
        Focus::Block => &app.block,
        _ => "",
    }
}

fn render_field(f: &mut Frame, app: &App, row: Rect, focus: Focus, label: &str) {
    let focused = app.focus == focus;
    let label_style = if focused {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    let value_style = if focused && app.editing {
        Style::default().fg(Color::Yellow)
    } else if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default()
    };

    let mut spans = vec![Span::styled(format!("{label:<7}: "), label_style)];
    if focus == Focus::Filter {
        spans.push(Span::styled(
            format!("< {} >", Resampling::ALL[app.filter].name()),
            value_style,
        ));
    } else if focus == Focus::Merge {
        spans.push(Span::styled(
            if app.merge { "< on >" } else { "< off >" },
            value_style,
        ));
    } else if focus == Focus::Quant {
        let step = QUANT_STEPS[app.quant_idx];
        spans.push(Span::styled(
            if step > 1 {
                format!("< {step} >")
            } else {
                "< off >".to_string()
            },
            value_style,
        ));
    } else {
        let value = value_of(app, focus);
        spans.push(Span::styled(value.to_string(), value_style));
        if focus == Focus::Input {
            if let Some(info) = &app.input_info {
                match info {
                    Ok((w, h)) => spans.push(Span::styled(
                        format!("  ok {w}x{h}"),
                        Style::default().fg(Color::Green),
                    )),
                    Err(_) => spans.push(Span::styled(
                        "  cannot read image",
                        Style::default().fg(Color::Red),
                    )),
                }
            }
        }
    }
    f.render_widget(Paragraph::new(Line::from(spans)), row);

    if focused && app.editing {
        let x = row.x + 9 + value_of(app, focus).chars().count() as u16;
        f.set_cursor_position((x.min(row.x + row.width.saturating_sub(1)), row.y));
    }
}

fn ui(f: &mut Frame, app: &App) {
    let chunks = Layout::vertical([
        Constraint::Length(11),
        Constraint::Min(3),
        Constraint::Length(3),
    ])
    .split(f.area());

    let form = Block::default()
        .borders(Borders::ALL)
        .title(" litbp-tui ")
        .title_bottom(Line::from(Span::styled(
            " Up/Down/Tab move, Enter edit/run, Left/Right switch, q quit ",
            Style::default().fg(Color::Yellow),
        )));
    let inner = form.inner(chunks[0]);
    f.render_widget(form, chunks[0]);

    let rows: [(Focus, &str); 8] = [
        (Focus::Input, "Input"),
        (Focus::Output, "Output"),
        (Focus::Scale, "Scale"),
        (Focus::Grid, "Grid"),
        (Focus::Block, "Block"),
        (Focus::Filter, "Filter"),
        (Focus::Merge, "Merge"),
        (Focus::Quant, "Quant"),
    ];
    for (i, (focus, label)) in rows.iter().enumerate() {
        let row = Rect {
            x: inner.x,
            y: inner.y + i as u16,
            width: inner.width,
            height: 1,
        };
        render_field(f, app, row, *focus, label);
    }

    let run_row = Rect {
        x: inner.x,
        y: inner.y + 8,
        width: inner.width,
        height: 1,
    };
    let buttons = if app.running {
        Line::from(Span::styled(
            "[ Running... ]",
            Style::default().fg(Color::DarkGray),
        ))
    } else {
        let button = |label: &'static str, focused: bool, color: Color| {
            if focused {
                Span::styled(
                    label,
                    Style::default()
                        .fg(Color::Black)
                        .bg(color)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(label, Style::default().fg(color))
            }
        };
        Line::from(vec![
            button("[ Run ]", app.focus == Focus::Run, Color::Green),
            Span::raw("  "),
            button("[ Copy ]", app.focus == Focus::Copy, Color::Cyan),
        ])
    };
    f.render_widget(
        Paragraph::new(buttons).alignment(Alignment::Center),
        run_row,
    );

    let log_block = Block::default().borders(Borders::ALL).title(" Log ");
    let log_height = log_block.inner(chunks[1]).height as usize;
    let lines: Vec<Line> = app
        .log
        .iter()
        .rev()
        .take(log_height)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|(s, c)| Line::from(Span::styled(s.clone(), Style::default().fg(*c))))
        .collect();
    f.render_widget(Paragraph::new(lines).block(log_block), chunks[1]);

    let (ratio, label) = match app.progress {
        Some((done, cells)) if cells > 0 => {
            (done as f64 / cells as f64, format!("{done}/{cells}"))
        }
        Some((done, cells)) => (0.0, format!("{done}/{cells}")),
        None => (0.0, "idle".to_string()),
    };
    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title(" Progress "))
        .gauge_style(Style::default().fg(Color::Cyan))
        .ratio(ratio)
        .label(label);
    f.render_widget(gauge, chunks[2]);
}

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    // Restore the terminal even on panic
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        default_hook(info);
    }));

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    if let Some(arg) = std::env::args().nth(1) {
        app.input = arg;
    }
    app.refresh_input_info();

    while !app.quit {
        terminal.draw(|f| ui(f, &app))?;
        if event::poll(Duration::from_millis(30))? {
            if let Event::Key(key) = event::read()? {
                app.on_key(key);
            }
        }
        app.drain_msgs();
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}
