//! clawdpilot — war room de agentes Claude: 4 PTYs con `claude` dentro, en una pantalla.

mod pane;

use std::path::PathBuf;

use anyhow::Result;
use ratatui::Frame;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers,
};
use ratatui::crossterm::execute;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use pane::{Pane, short_path};

const PANES: usize = 4;
const ACCENT: Color = Color::Rgb(217, 119, 87);
const DIM: Color = Color::DarkGray;

enum Mode {
    Normal,
    /// Se pulsó Ctrl+A; la siguiente tecla es un comando.
    Leader,
    /// Editando el directorio de trabajo del panel enfocado.
    Cwd(String),
}

struct App {
    panes: Vec<Pane>,
    focus: usize,
    zoom: bool,
    mode: Mode,
    quit: bool,
}

impl App {
    fn new(cwds: Vec<PathBuf>) -> Self {
        let here = cwds.first().cloned().unwrap_or_default();
        let panes = (0..PANES)
            .map(|i| Pane::new(cwds.get(i).cloned().unwrap_or_else(|| here.clone())))
            .collect();
        App { panes, focus: 0, zoom: false, mode: Mode::Normal, quit: false }
    }

    /// Rectángulos de cada panel: rejilla 2x2, o uno solo si hay zoom.
    fn layout(&self, area: Rect) -> Vec<Rect> {
        if self.zoom {
            return (0..PANES).map(|i| if i == self.focus { area } else { Rect::ZERO }).collect();
        }
        let half = Constraint::from_percentages([50, 50]);
        let rows = Layout::vertical(half.clone()).split(area);
        rows.iter().flat_map(|r| Layout::horizontal(half.clone()).split(*r).to_vec()).collect()
    }

    fn draw(&mut self, frame: &mut Frame) {
        let [body, footer] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());
        let slots = self.layout(body);
        let mut cursor = None;

        for (i, area) in slots.iter().enumerate() {
            if area.is_empty() {
                continue;
            }
            let focused = i == self.focus;
            let block = Block::bordered()
                .border_style(Style::default().fg(if focused { ACCENT } else { DIM }))
                .title(Line::from(vec![
                    Span::styled(" ✻ ", Style::default().fg(ACCENT)),
                    Span::styled(
                        format!("agent {}", i + 1),
                        Style::default().add_modifier(if focused {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                    ),
                    Span::styled(
                        format!(" · {} ", short_path(&self.panes[i].cwd)),
                        Style::default().fg(DIM),
                    ),
                ]));
            let inner = block.inner(*area);
            frame.render_widget(block, *area);

            if let Some(session) = self.panes[i].session.as_mut() {
                session.resize(inner.height, inner.width);
            }
            match self.panes[i].session.as_ref() {
                Some(session) => {
                    if let Some(pos) = session.render(inner, frame.buffer_mut())
                        && focused
                    {
                        cursor = Some(pos);
                    }
                }
                None => frame.render_widget(idle_card(focused), inner),
            }
        }

        if let Some(pos) = cursor {
            frame.set_cursor_position(pos);
        }
        frame.render_widget(self.footer(), footer);

        if let Mode::Cwd(input) = &self.mode {
            let popup = centered(frame.area(), 60, 3);
            frame.render_widget(Clear, popup);
            let block = Block::bordered()
                .border_style(Style::default().fg(ACCENT))
                .title(format!(" carpeta del agent {} ", self.focus + 1));
            let inner = block.inner(popup);
            frame.render_widget(block, popup);
            frame.render_widget(Paragraph::new(input.as_str()), inner);
            frame.set_cursor_position((
                inner.x + (input.chars().count() as u16).min(inner.width.saturating_sub(1)),
                inner.y,
            ));
        }
    }

    fn footer(&self) -> Line<'static> {
        let key = Style::default().fg(ACCENT);
        let txt = Style::default().fg(DIM);
        let hints: &[(&str, &str)] = match self.mode {
            Mode::Leader => &[
                ("1-4/Tab", "foco"),
                ("z", "zoom"),
                ("r", "reiniciar"),
                ("x", "matar"),
                ("c", "carpeta"),
                ("q", "salir"),
                ("^A", "literal"),
            ],
            Mode::Cwd(_) => &[("Enter", "confirmar"), ("Esc", "cancelar")],
            Mode::Normal if self.panes[self.focus].session.is_none() => {
                &[("Enter", "lanzar agente"), ("Tab", "siguiente panel"), ("^A", "comandos")]
            }
            // con un agente vivo el teclado es suyo: Tab, flechas y Esc van al PTY
            Mode::Normal => &[("^A", "comandos")],
        };
        let mut spans =
            vec![Span::styled(if matches!(self.mode, Mode::Leader) { " ^A ─ " } else { " " }, key)];
        for (k, label) in hints {
            spans.push(Span::styled(*k, key));
            spans.push(Span::styled(format!(" {}  ", label), txt));
        }
        Line::from(spans)
    }

    /// Tamaño interior del panel enfocado, para arrancar el PTY con la medida justa.
    fn focused_inner(&self, area: Rect) -> (u16, u16) {
        let [body, _] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);
        let rect = self.layout(body)[self.focus];
        (rect.height.saturating_sub(2).max(1), rect.width.saturating_sub(2).max(1))
    }

    fn launch_focused(&mut self, area: Rect) {
        let (rows, cols) = self.focused_inner(area);
        if let Err(e) = self.panes[self.focus].launch(rows, cols) {
            // sin `claude` en el PATH no hay nada que pilotar; mejor salir con el error visible
            self.quit = true;
            eprintln!("clawdpilot: no se pudo lanzar claude: {e}");
        }
    }

    fn on_key(&mut self, key: KeyEvent, area: Rect) {
        match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Cwd(mut input) => match key.code {
                KeyCode::Enter => {
                    let path = expand_home(input.trim());
                    if path.is_dir() {
                        self.panes[self.focus].kill();
                        self.panes[self.focus].cwd = path;
                    } else {
                        self.mode = Mode::Cwd(input); // ruta inválida: seguimos editando
                    }
                }
                KeyCode::Esc => {}
                KeyCode::Backspace => {
                    input.pop();
                    self.mode = Mode::Cwd(input);
                }
                KeyCode::Char(c) => {
                    input.push(c);
                    self.mode = Mode::Cwd(input);
                }
                _ => self.mode = Mode::Cwd(input),
            },

            Mode::Leader => match key.code {
                KeyCode::Char(c @ '1'..='4') => self.focus = c as usize - '1' as usize,
                KeyCode::Tab => self.focus = (self.focus + 1) % PANES,
                KeyCode::Char('z') => self.zoom = !self.zoom,
                KeyCode::Char('q') => self.quit = true,
                KeyCode::Char('x') => self.panes[self.focus].kill(),
                KeyCode::Char('r') => {
                    self.panes[self.focus].kill();
                    self.launch_focused(area);
                }
                KeyCode::Char('c') => {
                    self.mode = Mode::Cwd(short_path(&self.panes[self.focus].cwd));
                }
                // Ctrl+A Ctrl+A envía un Ctrl+A de verdad al agente
                _ => self.send_key(key),
            },

            Mode::Normal => {
                if key.code == KeyCode::Char('a') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.mode = Mode::Leader;
                } else if self.panes[self.focus].session.is_none() {
                    match key.code {
                        KeyCode::Enter => self.launch_focused(area),
                        KeyCode::Tab => self.focus = (self.focus + 1) % PANES,
                        _ => {}
                    }
                } else {
                    self.send_key(key);
                }
            }
        }
    }

    fn send_key(&mut self, key: KeyEvent) {
        if let (Some(session), Some(bytes)) =
            (self.panes[self.focus].session.as_mut(), encode_key(key))
        {
            session.send(&bytes);
        }
    }
}

fn idle_card(focused: bool) -> Paragraph<'static> {
    let accent = Style::default().fg(ACCENT).add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(DIM);
    Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled("✻", accent)),
        Line::from(Span::styled("Claude Code", dim)),
        Line::from(""),
        Line::from(Span::styled(
            if focused { "Enter para lanzar" } else { "" },
            Style::default().fg(if focused { ACCENT } else { DIM }),
        )),
    ])
    .centered()
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let [_, row, _] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(height), Constraint::Fill(1)])
            .areas(area);
    let [_, cell, _] =
        Layout::horizontal([Constraint::Fill(1), Constraint::Length(width), Constraint::Fill(1)])
            .areas(row);
    cell
}

fn expand_home(input: &str) -> PathBuf {
    match (input.strip_prefix("~"), std::env::var("HOME")) {
        (Some(rest), Ok(home)) => PathBuf::from(format!("{home}{rest}")),
        _ => PathBuf::from(input),
    }
}

/// Traduce una tecla de crossterm a los bytes que espera un terminal xterm.
fn encode_key(key: KeyEvent) -> Option<Vec<u8>> {
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let mut out = Vec::new();
    // prefijo Meta: ESC + la secuencia sin modificar
    let esc = |out: &mut Vec<u8>| {
        if alt {
            out.push(0x1b);
        }
    };
    // flechas con Alt usan el parámetro de modificador de xterm (;3)
    let arrow = |out: &mut Vec<u8>, final_byte: u8| {
        out.extend_from_slice(if alt { b"\x1b[1;3" } else { b"\x1b[" });
        out.push(final_byte);
    };

    match key.code {
        KeyCode::Char(c) => {
            esc(&mut out);
            if ctrl {
                out.push(match c {
                    ' ' | '@' => 0,
                    '?' => 0x7f,
                    'a'..='z' => c as u8 - 0x60,
                    '[' | '\\' | ']' | '^' | '_' => c as u8 - 0x40,
                    _ => return None,
                });
            } else {
                out.extend_from_slice(c.encode_utf8(&mut [0u8; 4]).as_bytes());
            }
        }
        KeyCode::Enter => {
            esc(&mut out);
            out.push(b'\r');
        }
        KeyCode::Backspace => {
            esc(&mut out);
            out.push(0x7f);
        }
        KeyCode::Tab => out.push(b'\t'),
        KeyCode::BackTab => out.extend_from_slice(b"\x1b[Z"),
        KeyCode::Esc => out.push(0x1b),
        KeyCode::Up => arrow(&mut out, b'A'),
        KeyCode::Down => arrow(&mut out, b'B'),
        KeyCode::Right => arrow(&mut out, b'C'),
        KeyCode::Left => arrow(&mut out, b'D'),
        KeyCode::Home => out.extend_from_slice(b"\x1b[H"),
        KeyCode::End => out.extend_from_slice(b"\x1b[F"),
        KeyCode::PageUp => out.extend_from_slice(b"\x1b[5~"),
        KeyCode::PageDown => out.extend_from_slice(b"\x1b[6~"),
        KeyCode::Delete => out.extend_from_slice(b"\x1b[3~"),
        KeyCode::Insert => out.extend_from_slice(b"\x1b[2~"),
        _ => return None,
    }
    Some(out)
}

/// `clawdpilot [dir1 [dir2 [dir3 [dir4]]]]` — sin argumentos, los cuatro
/// agentes comparten el directorio actual.
fn cwds_from_args() -> Result<Vec<PathBuf>> {
    let mut cwds = vec![];
    for arg in std::env::args().skip(1) {
        let path = expand_home(&arg).canonicalize().ok().filter(|p| p.is_dir());
        cwds.push(path.ok_or_else(|| anyhow::anyhow!("no es un directorio: {arg}"))?);
    }
    if cwds.len() > PANES {
        anyhow::bail!("como máximo {PANES} directorios, recibí {}", cwds.len());
    }
    if cwds.is_empty() {
        cwds.push(std::env::current_dir()?);
    }
    Ok(cwds)
}

fn main() -> Result<()> {
    let mut app = App::new(cwds_from_args()?);
    let mut terminal = ratatui::init();
    execute!(std::io::stdout(), EnableBracketedPaste)?;

    let result = run(&mut app, &mut terminal);

    let _ = execute!(std::io::stdout(), DisableBracketedPaste);
    ratatui::restore();
    result
}

fn run(app: &mut App, terminal: &mut ratatui::DefaultTerminal) -> Result<()> {
    while !app.quit {
        let area = terminal.draw(|f| app.draw(f))?.area;

        // 16 ms: refrescamos aunque no haya teclas, porque los PTYs escriben solos
        if event::poll(std::time::Duration::from_millis(16))? {
            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => app.on_key(key, area),
                Event::Paste(text) => {
                    if let Some(session) = app.panes[app.focus].session.as_mut() {
                        session.send(format!("\x1b[200~{text}\x1b[201~").as_bytes());
                    }
                }
                _ => {}
            }
        }
        app.panes.iter_mut().for_each(Pane::reap);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    /// Snapshot de la pantalla inicial: 4 paneles idle + footer.
    #[test]
    fn idle_grid_renders() {
        let mut app = App::new(vec![PathBuf::from("/tmp")]);
        let mut terminal = Terminal::new(TestBackend::new(76, 20)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();

        let screen: String = terminal
            .backend()
            .buffer()
            .content()
            .chunks(76)
            .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        println!("{screen}");

        assert_eq!(screen.matches('✻').count(), 8, "4 títulos + 4 tarjetas idle");
        assert!(screen.contains("agent 4"));
        assert!(screen.contains("Enter para lanzar"), "solo el panel enfocado lo muestra");
    }

    #[test]
    fn ctrl_a_is_a_leader_not_a_keystroke() {
        let mut app = App::new(vec![PathBuf::from("/tmp")]);
        let area = Rect::new(0, 0, 76, 20);
        let ctrl_a = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL);

        app.on_key(ctrl_a, area);
        assert!(matches!(app.mode, Mode::Leader));
        app.on_key(KeyEvent::from(KeyCode::Char('3')), area);
        assert_eq!(app.focus, 2);
        assert!(matches!(app.mode, Mode::Normal));

        app.on_key(ctrl_a, area);
        app.on_key(KeyEvent::from(KeyCode::Tab), area);
        assert_eq!(app.focus, 3, "^A Tab cicla; Tab a secas pertenece al agente");

        app.on_key(ctrl_a, area);
        app.on_key(KeyEvent::from(KeyCode::Char('z')), area);
        assert!(app.zoom);
    }

    #[test]
    fn zoom_gives_the_focused_pane_the_whole_area() {
        let mut app = App::new(vec![PathBuf::from("/tmp")]);
        let area = Rect::new(0, 0, 80, 24);
        assert_eq!(app.layout(area).iter().filter(|r| !r.is_empty()).count(), 4);

        app.focus = 2;
        app.zoom = true;
        let slots = app.layout(area);
        assert_eq!(slots[2], area);
        assert!(slots.iter().enumerate().all(|(i, r)| i == 2 || r.is_empty()));
    }
}
