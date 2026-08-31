//! Un panel = un agente de terminal (`claude`, `codex`, `aider`...) corriendo
//! dentro de un pseudo-terminal.
//!
//! El hilo lector vuelca los bytes del PTY en un parser vt100 compartido;
//! el hilo de render lee la grilla resultante y la pinta en ratatui.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::Result;
use portable_pty::{Child, CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

pub struct Session {
    parser: Arc<Mutex<vt100::Parser>>,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    size: (u16, u16),
}

impl Session {
    fn spawn(cmd: CommandBuilder, rows: u16, cols: u16) -> Result<Self> {
        let pair = NativePtySystem::default().openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave); // sin esto el PTY nunca ve EOF al morir el hijo

        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 0)));

        let sink = Arc::clone(&parser);
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                sink.lock().unwrap().process(&buf[..n]);
            }
        });

        Ok(Session { parser, writer, master: pair.master, child, size: (rows, cols) })
    }

    pub fn send(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        if self.size == (rows, cols) || rows == 0 || cols == 0 {
            return;
        }
        self.size = (rows, cols);
        let _ = self.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        self.parser.lock().unwrap().screen_mut().set_size(rows, cols);
    }

    /// ¿La aplicación del PTY pidió que le reportemos el ratón?
    pub fn wants_mouse(&self) -> bool {
        let parser = self.parser.lock().unwrap();
        let screen = parser.screen();
        // ponytail: solo SGR (1006), que es lo que negocian las TUIs de hoy
        screen.mouse_protocol_mode() != vt100::MouseProtocolMode::None
            && screen.mouse_protocol_encoding() == vt100::MouseProtocolEncoding::Sgr
    }

    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// Pinta la pantalla del PTY en `area`. Devuelve la posición absoluta del
    /// cursor si está visible, para que el panel enfocado muestre el caret real.
    pub fn render(&self, area: Rect, buf: &mut Buffer) -> Option<(u16, u16)> {
        let parser = self.parser.lock().unwrap();
        let screen = parser.screen();
        let (rows, cols) = screen.size();

        for y in 0..rows.min(area.height) {
            for x in 0..cols.min(area.width) {
                let Some(cell) = screen.cell(y, x) else { continue };
                let Some(target) = buf.cell_mut((area.x + x, area.y + y)) else { continue };
                if cell.is_wide_continuation() {
                    // ratatui espera símbolo vacío en la mitad derecha de un carácter ancho
                    target.set_symbol("");
                } else {
                    let s = cell.contents();
                    target.set_symbol(if s.is_empty() { " " } else { s });
                }
                target.set_style(cell_style(cell));
            }
        }

        if screen.hide_cursor() {
            return None;
        }
        let (cy, cx) = screen.cursor_position();
        (cy < area.height && cx < area.width).then(|| (area.x + cx, area.y + cy))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.kill();
    }
}

fn cell_style(cell: &vt100::Cell) -> Style {
    let mut style = Style::default().fg(color(cell.fgcolor())).bg(color(cell.bgcolor()));
    if cell.bold() {
        style = style.add_modifier(Modifier::BOLD);
    }
    if cell.italic() {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if cell.underline() {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    if cell.inverse() {
        style = style.add_modifier(Modifier::REVERSED);
    }
    style
}

fn color(c: vt100::Color) -> Color {
    match c {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

pub struct Pane {
    pub cwd: PathBuf,
    pub session: Option<Session>,
    /// Índice en [`agents`]: qué IA de terminal arranca este panel.
    agent: usize,
}

impl Pane {
    pub fn new(cwd: PathBuf) -> Self {
        Pane { cwd, session: None, agent: 0 }
    }

    /// Comando completo del agente, con sus argumentos.
    pub fn agent_cmd(&self) -> &'static str {
        &agents()[self.agent]
    }

    /// Nombre corto para el título: el programa, sin argumentos ni ruta.
    pub fn agent_name(&self) -> &'static str {
        let prog = self.agent_cmd().split_whitespace().next().unwrap_or("agent");
        prog.rsplit('/').next().unwrap_or(prog)
    }

    pub fn agent_index(&self) -> usize {
        self.agent
    }

    /// Elige otro agente de la lista. Mata la sesión: el cambio se ve al relanzar.
    pub fn set_agent(&mut self, i: usize) {
        if i >= agents().len() || i == self.agent {
            return;
        }
        self.kill();
        self.agent = i;
    }

    pub fn launch(&mut self, rows: u16, cols: u16) -> Result<()> {
        // ponytail: split por espacios, sin comillas; para algo más raro, un wrapper en el PATH
        let mut argv = self.agent_cmd().split_whitespace();
        let mut cmd = CommandBuilder::new(argv.next().unwrap_or("claude"));
        cmd.args(argv);
        cmd.cwd(&self.cwd);
        // vt100 no habla truecolor por índice; xterm-256color es lo que esperan estas TUIs
        cmd.env("TERM", "xterm-256color");
        self.session = Some(Session::spawn(cmd, rows.max(1), cols.max(1))?);
        Ok(())
    }

    pub fn kill(&mut self) {
        self.session = None; // Drop mata el hijo
    }

    /// Limpia la sesión si el proceso murió por su cuenta (`/exit`, crash).
    pub fn reap(&mut self) {
        if self.session.as_mut().is_some_and(|s| !s.is_alive()) {
            self.session = None;
        }
    }
}

/// Agentes disponibles, en el orden en que los cicla `^A a`. Se sobrescriben con
/// `CLAWDPILOT_AGENTS`, una lista separada por comas donde cada entrada es un
/// comando con sus argumentos: `CLAWDPILOT_AGENTS="claude --continue,codex,aider"`.
pub fn agents() -> &'static [String] {
    static AGENTS: OnceLock<Vec<String>> = OnceLock::new();
    AGENTS.get_or_init(|| parse_agents(std::env::var("CLAWDPILOT_AGENTS").ok()))
}

fn parse_agents(raw: Option<String>) -> Vec<String> {
    let listed: Vec<String> = raw
        .iter()
        .flat_map(|v| v.split(','))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if listed.is_empty() {
        ["claude", "codex", "agy", "aider"].map(String::from).to_vec()
    } else {
        listed
    }
}

pub fn short_path(p: &Path) -> String {
    let s = p.display().to_string();
    match std::env::var("HOME") {
        Ok(home) if s.starts_with(&home) => format!("~{}", &s[home.len()..]),
        _ => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain_until(session: &Session, needle: &str) -> String {
        for _ in 0..150 {
            std::thread::sleep(std::time::Duration::from_millis(20));
            let screen = session.parser.lock().unwrap().screen().contents();
            if screen.contains(needle) {
                return screen;
            }
        }
        panic!("nunca apareció {needle:?} en la pantalla del PTY");
    }

    /// El único trozo no trivial: PTY -> vt100 -> celdas de ratatui.
    #[test]
    fn pty_output_reaches_the_ratatui_buffer() {
        let mut cmd = CommandBuilder::new("printf");
        cmd.arg("hola mundo");
        let session = Session::spawn(cmd, 5, 20).unwrap();
        drain_until(&session, "hola mundo");

        // se pinta desplazado, para comprobar que respeta el origen del panel
        let area = Rect::new(2, 1, 20, 5);
        let mut buf = Buffer::empty(Rect::new(0, 0, 24, 7));
        session.render(area, &mut buf);

        let row: String = (0..24).map(|x| buf.cell((x, 1)).unwrap().symbol()).collect();
        assert_eq!(row.trim_end(), "  hola mundo");
        assert_eq!(buf.cell((0, 0)).unwrap().symbol(), " ", "no debe pintar fuera del área");
    }

    #[test]
    fn agent_list_falls_back_to_the_defaults() {
        assert_eq!(
            parse_agents(Some("codex, aider --model x ,,".into())),
            ["codex", "aider --model x"]
        );
        assert_eq!(parse_agents(Some("  ,, ".into()))[0], "claude");
        assert!(parse_agents(None).len() > 1, "hay algo que ciclar con ^A a");
    }

    #[test]
    fn set_agent_picks_from_the_list_and_ignores_the_impossible() {
        let mut pane = Pane::new(PathBuf::from("/tmp"));
        let first = pane.agent_cmd();

        pane.set_agent(agents().len() - 1);
        assert_ne!(pane.agent_cmd(), first);
        assert_eq!(pane.agent_index(), agents().len() - 1);

        pane.set_agent(agents().len()); // fuera de rango: se queda como estaba
        assert_eq!(pane.agent_index(), agents().len() - 1);

        pane.agent = agents().iter().position(|a| a.contains(' ')).unwrap_or(0);
        assert!(!pane.agent_name().contains(' '), "el título no lleva argumentos");
    }

    /// Prueba manual contra el `claude` real: `cargo test -- --ignored --nocapture`
    #[test]
    #[ignore = "lanza el binario claude de verdad"]
    fn real_claude_tui_renders() {
        let mut pane = Pane::new(std::env::temp_dir());
        pane.launch(30, 100).unwrap();
        let screen = drain_until(pane.session.as_ref().unwrap(), "Claude Code");
        println!("{screen}");
    }
}
