//! clawdpilot — war room de agentes de terminal: N PTYs con `claude`, `codex`,
//! `aider`... dentro, en una pantalla.

mod pane;
mod sidebar;

use std::path::PathBuf;

use anyhow::Result;
use ratatui::Frame;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use pane::{Pane, agents, short_path};
use sidebar::{Item, Sidebar};

const DEFAULT_PANES: usize = 4;
/// Debajo de esto un panel no alcanza para leer nada: se rechaza crear más.
const MIN_PANE_W: u16 = 20;
const MIN_PANE_H: u16 = 6;
pub const ACCENT: Color = Color::Rgb(217, 119, 87);
pub const DIM: Color = Color::DarkGray;

enum Mode {
    Normal,
    /// Se pulsó Ctrl+A; la siguiente tecla es un comando.
    Leader,
    /// Editando el directorio de trabajo del panel enfocado.
    Cwd(String),
    /// Eligiendo agente para el panel enfocado; guarda la fila resaltada.
    Agents(usize),
}

struct App {
    panes: Vec<Pane>,
    focus: usize,
    zoom: bool,
    mode: Mode,
    quit: bool,
    sidebar: Sidebar,
    /// Fotogramas dibujados; le da el ritmo al spinner de actividad.
    tick: u64,
    /// Aviso efímero en el footer; se borra con la siguiente tecla.
    notice: Option<String>,
}

impl App {
    fn new(cwds: Vec<PathBuf>) -> Self {
        let here = cwds.first().cloned().unwrap_or_default();
        let panes = (0..cwds.len().max(DEFAULT_PANES))
            .map(|i| Pane::new(cwds.get(i).cloned().unwrap_or_else(|| here.clone())))
            .collect();
        App {
            panes,
            focus: 0,
            zoom: false,
            mode: Mode::Normal,
            quit: false,
            sidebar: Sidebar::new(),
            tick: 0,
            notice: None,
        }
    }

    /// Reparte la pantalla: barra lateral, rejilla de agentes y footer.
    fn chrome(&self, area: Rect) -> (Rect, Rect, Rect) {
        let [main, footer] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);
        let [side, body] = Layout::horizontal([
            Constraint::Length(self.sidebar.width(area.width)),
            Constraint::Fill(1),
        ])
        .areas(main);
        (side, body, footer)
    }

    fn body(&self, area: Rect) -> Rect {
        self.chrome(area).1
    }

    /// Rectángulos de cada panel: rejilla lo más cuadrada posible, o uno solo si hay zoom.
    fn layout(&self, area: Rect) -> Vec<Rect> {
        let n = self.panes.len();
        if self.zoom {
            return (0..n).map(|i| if i == self.focus { area } else { Rect::ZERO }).collect();
        }
        let (cols, rows) = grid_shape(n);
        let bands = Layout::vertical(vec![Constraint::Fill(1); rows]).split(area);
        let mut slots = Vec::with_capacity(n);
        for (r, band) in bands.iter().enumerate() {
            // la última fila reparte a lo ancho solo los paneles que le quedan
            let in_row = (n - r * cols).min(cols);
            slots.extend(
                Layout::horizontal(vec![Constraint::Fill(1); in_row]).split(*band).iter().copied(),
            );
        }
        slots
    }

    fn draw(&mut self, frame: &mut Frame) {
        let (side, body, footer) = self.chrome(frame.area());
        self.sidebar.render(side, frame, &self.panes, self.focus, self.tick);
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
                    Span::styled(
                        format!(" {} ", i + 1),
                        Style::default()
                            .fg(if focused { ACCENT } else { DIM })
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        self.panes[i].agent_name(),
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
                None => frame.render_widget(idle_card(self.panes[i].agent_name(), focused), inner),
            }
        }

        if let Some(pos) = cursor {
            frame.set_cursor_position(pos);
        }
        frame.render_widget(self.footer(), footer);

        if let Mode::Agents(sel) = self.mode {
            let popup = agents_popup(frame.area());
            frame.render_widget(Clear, popup);
            let block = Block::bordered()
                .border_style(Style::default().fg(ACCENT))
                .title(format!(" agente del panel {} ", self.focus + 1));
            let inner = block.inner(popup);
            frame.render_widget(block, popup);

            let current = self.panes[self.focus].agent_index();
            let rows: Vec<Line> = agents()
                .iter()
                .enumerate()
                .map(|(i, cmd)| {
                    let mark = if i == current { "✻ " } else { "  " };
                    let style = if i == sel {
                        Style::default().fg(ACCENT).add_modifier(Modifier::REVERSED)
                    } else if i == current {
                        Style::default().fg(ACCENT)
                    } else {
                        Style::default()
                    };
                    Line::from(Span::styled(format!("{mark}{cmd}"), style))
                })
                .collect();
            frame.render_widget(Paragraph::new(rows), inner);
        }

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
            // `x` mata al agente vivo; sobre un panel en reposo, lo cierra
            Mode::Leader if self.panes[self.focus].session.is_some() => &[
                ("1-9/Tab", "foco"),
                ("n", "nuevo"),
                ("z", "zoom"),
                ("r", "reiniciar"),
                ("x", "matar"),
                ("c", "carpeta"),
                ("a", "agente"),
                ("b", "lateral"),
                ("q", "salir"),
            ],
            Mode::Leader => &[
                ("1-9/Tab", "foco"),
                ("n", "nuevo"),
                ("z", "zoom"),
                ("x", "cerrar panel"),
                ("c", "carpeta"),
                ("a", "agente"),
                ("b", "lateral"),
                ("q", "salir"),
                ("^A", "literal"),
            ],
            Mode::Cwd(_) => &[("Enter", "confirmar"), ("Esc", "cancelar")],
            Mode::Agents(_) => {
                &[("↑↓/click", "elegir"), ("Enter", "confirmar"), ("Esc", "cancelar")]
            }
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
        if let Some(notice) = &self.notice {
            spans.push(Span::styled(format!("· {notice}"), Style::default().fg(Color::Yellow)));
        }
        Line::from(spans)
    }

    /// Añade un panel al final, heredando la carpeta del enfocado, y le da el foco.
    fn add_pane(&mut self, area: Rect) {
        if !fits(self.panes.len() + 1, self.body(area)) {
            self.notice = Some("no cabe otro panel en esta ventana".into());
            return;
        }
        let cwd = self.panes[self.focus].cwd.clone();
        self.panes.push(Pane::new(cwd));
        self.focus = self.panes.len() - 1;
        self.zoom = false;
    }

    /// Cierra el panel enfocado. Siempre queda al menos uno.
    fn close_pane(&mut self) {
        if self.panes.len() == 1 {
            self.notice = Some("no puedes cerrar el último panel".into());
            return;
        }
        self.panes.remove(self.focus); // Drop mata la sesión si la hubiera
        self.focus = self.focus.min(self.panes.len() - 1);
    }

    /// Tamaño interior del panel enfocado, para arrancar el PTY con la medida justa.
    fn focused_inner(&self, area: Rect) -> (u16, u16) {
        let rect = self.layout(self.body(area))[self.focus];
        (rect.height.saturating_sub(2).max(1), rect.width.saturating_sub(2).max(1))
    }

    fn launch_focused(&mut self, area: Rect) {
        let (rows, cols) = self.focused_inner(area);
        if self.panes[self.focus].launch(rows, cols).is_err() {
            // el agente puede no estar instalado: se avisa y se sigue, hay otros en la lista
            self.notice =
                Some(format!("no se pudo lanzar `{}`", self.panes[self.focus].agent_cmd()));
        }
    }

    fn on_key(&mut self, key: KeyEvent, area: Rect) {
        self.notice = None;
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

            Mode::Agents(sel) => {
                let last = agents().len() - 1;
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.mode = Mode::Agents(if sel == 0 { last } else { sel - 1 });
                    }
                    KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                        self.mode = Mode::Agents(if sel == last { 0 } else { sel + 1 });
                    }
                    KeyCode::Enter => self.panes[self.focus].set_agent(sel),
                    KeyCode::Esc => {}
                    _ => self.mode = Mode::Agents(sel),
                }
            }

            Mode::Leader => match key.code {
                KeyCode::Char(c @ '1'..='9') => {
                    let i = c as usize - '1' as usize;
                    if i < self.panes.len() {
                        self.focus = i;
                    }
                }
                KeyCode::Tab => self.focus = (self.focus + 1) % self.panes.len(),
                KeyCode::Char('n') => self.act(Item::NewPane, area),
                KeyCode::Char('z') => self.act(Item::Zoom, area),
                KeyCode::Char('q') => self.act(Item::Quit, area),
                KeyCode::Char('x') => self.act(Item::Kill, area),
                KeyCode::Char('r') => self.act(Item::Restart, area),
                KeyCode::Char('c') => self.act(Item::Cwd, area),
                KeyCode::Char('b') => self.act(Item::Logo, area),
                // el guard deja pasar ^A ^A al agente
                KeyCode::Char('a') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.act(Item::Agent, area)
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
                        KeyCode::Tab => self.focus = (self.focus + 1) % self.panes.len(),
                        _ => {}
                    }
                } else {
                    self.send_key(key);
                }
            }
        }
    }

    /// Un solo sitio donde ocurren las cosas: lo llaman igual el teclado y el sidebar.
    fn act(&mut self, item: Item, area: Rect) {
        match item {
            Item::Logo => self.sidebar.toggle(),
            Item::Pane(i) => self.focus = i.min(self.panes.len() - 1),
            Item::NewPane => self.add_pane(area),
            Item::Launch => self.launch_focused(area),
            Item::Restart => {
                self.panes[self.focus].kill();
                self.launch_focused(area);
            }
            // sobre un agente vivo lo mata; sobre un panel en reposo, cierra el panel
            Item::Kill => {
                if self.panes[self.focus].session.is_some() {
                    self.panes[self.focus].kill();
                } else {
                    self.close_pane();
                }
            }
            Item::Cwd => self.mode = Mode::Cwd(short_path(&self.panes[self.focus].cwd)),
            Item::Zoom => self.zoom = !self.zoom,
            Item::Agent => self.mode = Mode::Agents(self.panes[self.focus].agent_index()),
            Item::Quit => self.quit = true,
        }
    }

    /// El ratón enfoca paneles, elige en la lista de agentes y, si el agente pidió
    /// que le reportemos el ratón, se lo pasamos tal cual.
    fn on_mouse(&mut self, m: MouseEvent, area: Rect) {
        let point = Rect::new(m.column, m.row, 1, 1);
        let inside = |r: Rect| r.union(point) == r;

        if let Mode::Agents(sel) = self.mode {
            let popup = agents_popup(area);
            let row =
                m.row.checked_sub(popup.y + 1).map(usize::from).filter(|i| *i < agents().len());
            match m.kind {
                MouseEventKind::ScrollUp => {
                    self.mode = Mode::Agents(sel.saturating_sub(1));
                }
                MouseEventKind::ScrollDown => {
                    self.mode = Mode::Agents((sel + 1).min(agents().len() - 1));
                }
                MouseEventKind::Down(MouseButton::Left) if inside(popup) => match row {
                    // un clic sobre una fila la elige y cierra: no hace falta el Enter
                    Some(i) => {
                        self.panes[self.focus].set_agent(i);
                        self.mode = Mode::Normal;
                    }
                    None => self.mode = Mode::Agents(sel),
                },
                // clic fuera del popup: cancelar
                MouseEventKind::Down(_) => self.mode = Mode::Normal,
                _ => {}
            }
            return;
        }

        let (side, body, _) = self.chrome(area);
        if inside(side) {
            let over = self.sidebar.hit(side, m.column, m.row, &self.panes, self.focus);
            self.sidebar.hover = over;
            if let (MouseEventKind::Down(MouseButton::Left), Some(item)) = (m.kind, over) {
                self.notice = None;
                self.act(item, area);
            }
            return;
        }
        self.sidebar.hover = None;

        let slots = self.layout(body);
        let Some(i) = slots.iter().position(|r| !r.is_empty() && inside(*r)) else { return };
        if matches!(m.kind, MouseEventKind::Down(_)) {
            self.notice = None;
            self.focus = i;
            self.mode = Mode::Normal;
            // el título lleva el nombre del agente: clic ahí = abrir la lista
            if m.row == slots[i].y {
                self.mode = Mode::Agents(self.panes[i].agent_index());
                return;
            }
        }
        if i != self.focus {
            return; // el resto de eventos son del panel enfocado, no del que se sobrevuela
        }
        // coordenadas dentro del panel, descontando el borde
        let (Some(col), Some(row)) =
            (m.column.checked_sub(slots[i].x + 1), m.row.checked_sub(slots[i].y + 1))
        else {
            return;
        };
        if let Some(session) = self.panes[i].session.as_mut()
            && session.wants_mouse()
            && let Some(bytes) = encode_mouse(m, col, row)
        {
            session.send(&bytes);
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

fn idle_card(agent: &'static str, focused: bool) -> Paragraph<'static> {
    let accent = Style::default().fg(ACCENT).add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(DIM);
    Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled("✻", accent)),
        Line::from(Span::styled(agent, dim)),
        Line::from(""),
        Line::from(Span::styled(
            if focused { "Enter para lanzar" } else { "" },
            Style::default().fg(if focused { ACCENT } else { DIM }),
        )),
    ])
    .centered()
}

/// Columnas y filas para `n` paneles, tan cuadrado como se pueda: 4→2x2, 6→3x2, 9→3x3.
fn grid_shape(n: usize) -> (usize, usize) {
    let cols = (n as f64).sqrt().ceil() as usize;
    (cols.max(1), n.div_ceil(cols.max(1)).max(1))
}

/// ¿Caben `n` paneles en `area` sin dejarlos ilegibles?
fn fits(n: usize, area: Rect) -> bool {
    let (cols, rows) = grid_shape(n);
    area.width / cols as u16 >= MIN_PANE_W && area.height / rows as u16 >= MIN_PANE_H
}

/// Recuadro de la lista de agentes. Lo comparten el dibujo y el ratón, así que la
/// fila que ves es exactamente la que se clica.
fn agents_popup(area: Rect) -> Rect {
    let widest = agents().iter().map(|a| a.chars().count()).max().unwrap_or(10) as u16;
    // el mínimo es lo que mide el título del recuadro
    centered(
        area,
        (widest + 8).max(24).min(area.width),
        (agents().len() as u16 + 2).min(area.height),
    )
}

/// Traduce un evento de ratón al reporte SGR que espera una TUI (`\x1b[<b;col;rowM`).
/// Coordenadas relativas al interior del panel, base 1.
fn encode_mouse(m: MouseEvent, col: u16, row: u16) -> Option<Vec<u8>> {
    let button = |b| match b {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    };
    let (mut code, press) = match m.kind {
        MouseEventKind::Down(b) => (button(b), true),
        MouseEventKind::Up(b) => (button(b), false),
        MouseEventKind::Drag(b) => (button(b) + 32, true),
        MouseEventKind::ScrollUp => (64, true),
        MouseEventKind::ScrollDown => (65, true),
        MouseEventKind::ScrollLeft => (66, true),
        MouseEventKind::ScrollRight => (67, true),
        MouseEventKind::Moved => return None,
    };
    for (modifier, bit) in
        [(KeyModifiers::SHIFT, 4), (KeyModifiers::ALT, 8), (KeyModifiers::CONTROL, 16)]
    {
        if m.modifiers.contains(modifier) {
            code += bit;
        }
    }
    let end = if press { 'M' } else { 'm' };
    Some(format!("\x1b[<{code};{};{}{end}", col + 1, row + 1).into_bytes())
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

/// `clawdpilot [dir...]` — un panel por directorio, con un mínimo de
/// `DEFAULT_PANES`. Sin argumentos, todos comparten el directorio actual.
fn cwds_from_args() -> Result<Vec<PathBuf>> {
    let mut cwds = vec![];
    for arg in std::env::args().skip(1) {
        let path = expand_home(&arg).canonicalize().ok().filter(|p| p.is_dir());
        cwds.push(path.ok_or_else(|| anyhow::anyhow!("no es un directorio: {arg}"))?);
    }
    if cwds.is_empty() {
        cwds.push(std::env::current_dir()?);
    }
    Ok(cwds)
}

fn main() -> Result<()> {
    let mut app = App::new(cwds_from_args()?);
    let mut terminal = ratatui::init();
    execute!(std::io::stdout(), EnableBracketedPaste, EnableMouseCapture)?;

    let result = run(&mut app, &mut terminal);

    let _ = execute!(std::io::stdout(), DisableBracketedPaste, DisableMouseCapture);
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
                Event::Mouse(m) => app.on_mouse(m, area),
                Event::Paste(text) => {
                    if let Some(session) = app.panes[app.focus].session.as_mut() {
                        session.send(format!("\x1b[200~{text}\x1b[201~").as_bytes());
                    }
                }
                _ => {}
            }
        }
        app.panes.iter_mut().for_each(Pane::tick);
        app.tick += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

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

        assert_eq!(screen.matches('✻').count(), 5, "el logo + una tarjeta idle por panel");
        assert!(screen.contains(" 4 claude"), "el título lleva el número y el agente");
        assert!(screen.contains("c l a w d p i l o t"), "el logo se queda mientras esté abierto");
        assert!(screen.contains("▸1 claude"), "y el sidebar marca el panel enfocado");
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

        let before = app.panes[app.focus].agent_cmd();
        app.on_key(ctrl_a, area);
        app.on_key(KeyEvent::from(KeyCode::Char('a')), area);
        assert!(matches!(app.mode, Mode::Agents(_)), "^A a abre la lista de agentes");
        app.on_key(KeyEvent::from(KeyCode::Down), area);
        app.on_key(KeyEvent::from(KeyCode::Enter), area);
        assert_ne!(app.panes[app.focus].agent_cmd(), before, "y elegir cambia de IA");
        assert!(matches!(app.mode, Mode::Normal));
    }

    /// La rejilla se recalcula al crecer y ninguna celda se solapa ni se sale.
    #[test]
    fn grid_reshapes_as_panes_are_added() {
        assert_eq!(grid_shape(1), (1, 1));
        assert_eq!(grid_shape(2), (2, 1));
        assert_eq!(grid_shape(4), (2, 2));
        assert_eq!(grid_shape(6), (3, 2));
        assert_eq!(grid_shape(9), (3, 3));

        let area = Rect::new(0, 0, 150, 60);
        for n in 1..=9 {
            let mut app = App::new(vec![PathBuf::from("/tmp")]);
            app.panes.resize_with(n, || Pane::new(PathBuf::from("/tmp")));
            let slots = app.layout(area);

            assert_eq!(slots.len(), n, "un rectángulo por panel");
            let covered: u32 = slots.iter().map(|r| r.area()).sum();
            assert_eq!(covered, area.area(), "la rejilla cubre todo sin solaparse ({n})");
            assert!(slots.iter().all(|r| area.union(*r) == area), "ninguna celda se sale ({n})");
        }
    }

    #[test]
    fn new_pane_inherits_the_cwd_and_takes_focus() {
        let mut app = App::new(vec![PathBuf::from("/tmp")]);
        let area = Rect::new(0, 0, 150, 60);
        let ctrl_a = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL);

        app.panes[0].cwd = PathBuf::from("/usr");
        app.on_key(ctrl_a, area);
        app.on_key(KeyEvent::from(KeyCode::Char('n')), area);

        assert_eq!(app.panes.len(), 5);
        assert_eq!(app.focus, 4, "el panel nuevo queda enfocado");
        assert_eq!(app.panes[4].cwd, PathBuf::from("/usr"), "hereda la carpeta del enfocado");
    }

    #[test]
    fn refuses_to_add_a_pane_that_would_not_fit() {
        let mut app = App::new(vec![PathBuf::from("/tmp")]);
        // 50 columnas dan dos paneles de 25, pero no los tres de 16 que pediría un quinto
        let area = Rect::new(0, 0, 50, 25);
        assert!(fits(4, app.body(area)) && !fits(5, app.body(area)));

        let ctrl_a = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL);
        app.on_key(ctrl_a, area);
        app.on_key(KeyEvent::from(KeyCode::Char('n')), area);

        assert_eq!(app.panes.len(), 4, "no se añade");
        assert!(app.notice.is_some(), "y el footer lo dice en vez de callarse");
    }

    /// `^A x` mata al agente vivo; repetido sobre el panel en reposo, lo cierra.
    #[test]
    fn x_closes_an_idle_pane_but_never_the_last_one() {
        let mut app = App::new(vec![PathBuf::from("/tmp")]);
        let area = Rect::new(0, 0, 150, 60);
        let ctrl_a = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL);
        let close = |app: &mut App| {
            app.on_key(ctrl_a, area);
            app.on_key(KeyEvent::from(KeyCode::Char('x')), area);
        };

        app.focus = 1;
        app.panes[1].cwd = PathBuf::from("/usr");
        close(&mut app);
        assert_eq!(app.panes.len(), 3);
        assert!(app.panes.iter().all(|p| p.cwd != Path::new("/usr")), "cerró el enfocado");

        for _ in 0..5 {
            close(&mut app);
        }
        assert_eq!(app.panes.len(), 1, "el último panel no se cierra");
        assert_eq!(app.focus, 0);
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

    /// La lista se dibuja encima de la rejilla y marca el agente en uso.
    #[test]
    fn agent_list_renders_over_the_grid() {
        let mut app = App::new(vec![PathBuf::from("/tmp")]);
        app.mode = Mode::Agents(1);
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

        assert!(screen.contains("agente del panel 1"));
        for agent in agents() {
            assert!(screen.contains(agent.as_str()), "falta {agent} en la lista");
        }
        assert!(screen.contains("✻ claude"), "el agente en uso va marcado");
    }

    /// Clica la fila del sidebar donde vive `item`, buscándola con el mismo
    /// hit-test que usa la aplicación.
    fn click_item(app: &mut App, item: Item, area: Rect) {
        let (side, ..) = app.chrome(area);
        let y = (side.y..side.bottom())
            .find(|&y| app.sidebar.hit(side, side.x + 1, y, &app.panes, app.focus) == Some(item))
            .unwrap_or_else(|| panic!("{item:?} no aparece en el sidebar"));
        click(app, MouseEventKind::Down(MouseButton::Left), side.x + 1, y, area);
    }

    /// Cada fila del sidebar hace exactamente lo que su atajo.
    #[test]
    fn the_sidebar_does_what_the_shortcuts_do() {
        let mut app = App::new(vec![PathBuf::from("/tmp")]);
        let area = Rect::new(0, 0, 120, 40);

        click_item(&mut app, Item::Pane(2), area);
        assert_eq!(app.focus, 2, "clic en la lista enfoca ese panel");

        click_item(&mut app, Item::NewPane, area);
        assert_eq!(app.panes.len(), 5);
        assert_eq!(app.focus, 4);

        click_item(&mut app, Item::Zoom, area);
        assert!(app.zoom);

        click_item(&mut app, Item::Agent, area);
        assert!(matches!(app.mode, Mode::Agents(_)));
        app.mode = Mode::Normal;

        click_item(&mut app, Item::Kill, area);
        assert_eq!(app.panes.len(), 4, "sobre un panel en reposo, cierra el panel");

        click_item(&mut app, Item::Quit, area);
        assert!(app.quit);
    }

    /// Lo que no se puede hacer no se puede clicar: sin agente vivo no hay reiniciar.
    #[test]
    fn actions_that_make_no_sense_are_not_clickable() {
        let mut app = App::new(vec![PathBuf::from("/tmp")]);
        app.panes.truncate(1);
        let area = Rect::new(0, 0, 120, 40);
        let (side, ..) = app.chrome(area);
        let rows = || {
            (side.y..side.bottom())
                .filter_map(|y| app.sidebar.hit(side, side.x + 1, y, &app.panes, app.focus))
                .collect::<Vec<_>>()
        };

        assert!(rows().contains(&Item::Launch), "en reposo se puede lanzar");
        assert!(!rows().contains(&Item::Restart), "pero no reiniciar, no hay nada corriendo");
        assert!(!rows().contains(&Item::Kill), "ni cerrar: es el último panel que queda");
    }

    /// El logo pliega y despliega, y una ventana estrecha lo pliega sola.
    #[test]
    fn the_sidebar_folds_into_a_dock() {
        let mut app = App::new(vec![PathBuf::from("/tmp")]);
        let area = Rect::new(0, 0, 120, 40);
        assert_eq!(app.chrome(area).0.width, sidebar::WIDE);

        click_item(&mut app, Item::Logo, area);
        assert_eq!(app.chrome(area).0.width, sidebar::DOCK);
        click_item(&mut app, Item::Logo, area);
        assert_eq!(app.chrome(area).0.width, sidebar::WIDE);

        let narrow = Rect::new(0, 0, sidebar::NARROW_WINDOW - 1, 40);
        assert_eq!(app.chrome(narrow).0.width, sidebar::DOCK, "sin pedirlo, para no ahogar");
    }

    #[test]
    #[ignore = "solo para regenerar el mockup del README"]
    fn readme_screenshot() {
        let mut app = App::new(vec![
            PathBuf::from("/tmp/api"),
            PathBuf::from("/tmp/web"),
            PathBuf::from("/tmp/docs"),
        ]);
        app.panes[1].set_agent(1);
        app.panes[2].set_agent(2);
        app.focus = 0;
        let mut terminal = Terminal::new(TestBackend::new(96, 26)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        let screen: String = terminal
            .backend()
            .buffer()
            .content()
            .chunks(96)
            .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        println!("{screen}");
    }

    /// Plegado se queda el logo y los estados, sin etiquetas.
    #[test]
    fn dock_keeps_the_logo_and_the_status_dots() {
        let mut app = App::new(vec![PathBuf::from("/tmp")]);
        app.sidebar.toggle();
        let mut terminal = Terminal::new(TestBackend::new(60, 16)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();

        let screen: String = terminal
            .backend()
            .buffer()
            .content()
            .chunks(60)
            .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        println!("{screen}");

        assert!(screen.contains('✻'), "el logo se queda");
        assert!(!screen.contains("c l a w d"), "el nombre no");
        assert_eq!(screen.matches('○').count(), 4, "un punto de estado por panel");
    }

    fn click(app: &mut App, kind: MouseEventKind, x: u16, y: u16, area: Rect) {
        app.on_mouse(
            MouseEvent { kind, column: x, row: y, modifiers: KeyModifiers::empty() },
            area,
        );
    }

    /// El ratón enfoca el panel que se clica, y su barra de título abre la lista.
    #[test]
    fn clicking_focuses_a_pane_and_its_title_opens_the_agent_list() {
        let mut app = App::new(vec![PathBuf::from("/tmp")]);
        let area = Rect::new(0, 0, 80, 25);
        let bottom_right = app.layout(app.body(area))[3];

        click(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            bottom_right.x + 5,
            area.height - 3,
            area,
        );
        assert_eq!(app.focus, 3);
        assert!(matches!(app.mode, Mode::Normal), "el cuerpo del panel solo enfoca");

        click(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            bottom_right.x + 5,
            bottom_right.y,
            area,
        );
        assert!(matches!(app.mode, Mode::Agents(_)), "el título abre la lista");
    }

    /// Y dentro de la lista, la fila que se ve es la que se elige.
    #[test]
    fn clicking_a_row_picks_that_agent() {
        let mut app = App::new(vec![PathBuf::from("/tmp")]);
        let area = Rect::new(0, 0, 80, 25);
        let popup = agents_popup(area);
        app.mode = Mode::Agents(0);

        let last = agents().len() - 1;
        click(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            popup.x + 3,
            popup.y + 1 + last as u16,
            area,
        );
        assert!(matches!(app.mode, Mode::Normal), "elegir cierra la lista");
        assert_eq!(app.panes[0].agent_index(), last);

        app.mode = Mode::Agents(0);
        click(&mut app, MouseEventKind::Down(MouseButton::Left), 0, 0, area);
        assert!(matches!(app.mode, Mode::Normal), "clic fuera cancela");
    }

    #[test]
    fn mouse_reports_are_sgr_and_relative_to_the_pane() {
        let ev = |kind| MouseEvent { kind, column: 0, row: 0, modifiers: KeyModifiers::empty() };
        let sgr = |kind, c, r| String::from_utf8(encode_mouse(ev(kind), c, r).unwrap()).unwrap();

        assert_eq!(sgr(MouseEventKind::Down(MouseButton::Left), 0, 0), "\x1b[<0;1;1M");
        assert_eq!(sgr(MouseEventKind::Up(MouseButton::Right), 9, 4), "\x1b[<2;10;5m");
        assert_eq!(sgr(MouseEventKind::ScrollUp, 0, 0), "\x1b[<64;1;1M");
        assert!(encode_mouse(ev(MouseEventKind::Moved), 0, 0).is_none(), "no se reporta el vuelo");

        let ctrl_scroll = MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::CONTROL,
        };
        assert_eq!(
            String::from_utf8(encode_mouse(ctrl_scroll, 0, 0).unwrap()).unwrap(),
            "\x1b[<81;1;1M"
        );
    }
}
