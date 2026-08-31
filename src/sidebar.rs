//! Panel lateral: identidad, estado de los agentes y acciones a un clic.
//!
//! Cada fila que se dibuja lleva pegado su [`Item`], así que el mismo cálculo
//! sirve para pintar y para saber qué se clicó: lo que ves es lo que se toca.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::pane::Pane;
use crate::{ACCENT, DIM};

/// Ancho con etiquetas y el ancho plegado, solo iconos.
pub const WIDE: u16 = 24;
pub const DOCK: u16 = 6;
/// Por debajo de esto el sidebar ancho ahoga a los agentes: arranca plegado.
pub const NARROW_WINDOW: u16 = 60;

const HOVER: Color = Color::Rgb(48, 46, 44);
const SPIN: [&str; 8] = ["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"];

/// Todo lo que se puede clicar en la barra.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Item {
    Logo,
    Pane(usize),
    NewPane,
    Launch,
    Restart,
    Kill,
    Cwd,
    Zoom,
    Agent,
    Quit,
}

/// Acciones sobre el panel enfocado, en el orden en que se listan.
const ACTIONS: [(Item, &str, &str, &str); 6] = [
    (Item::Launch, "▶", "lanzar", "Enter"),
    (Item::Restart, "⟳", "reiniciar", "^A r"),
    (Item::Kill, "■", "detener", "^A x"),
    (Item::Cwd, "⌂", "carpeta", "^A c"),
    (Item::Zoom, "⤢", "zoom", "^A z"),
    (Item::Agent, "✻", "agente", "^A a"),
];

pub struct Sidebar {
    pub collapsed: bool,
    /// Fila bajo el puntero, para resaltarla.
    pub hover: Option<Item>,
}

struct Row {
    item: Option<Item>,
    line: Line<'static>,
    enabled: bool,
}

/// Con este ancho ya no caben etiquetas: se dibuja como dock de iconos.
fn is_dock(width: u16) -> bool {
    width < WIDE - 4
}

impl Sidebar {
    pub fn new() -> Self {
        Sidebar { collapsed: false, hover: None }
    }

    /// Ancho que ocupa. Una ventana estrecha lo pliega aunque no lo pidas.
    pub fn width(&self, window: u16) -> u16 {
        if self.collapsed || window < NARROW_WINDOW { DOCK } else { WIDE }
    }

    pub fn toggle(&mut self) {
        self.collapsed = !self.collapsed;
        self.hover = None;
    }

    /// ¿Tiene sentido la acción sobre el panel enfocado? Lo que no, se pinta apagado.
    fn enabled(&self, item: Item, panes: &[Pane], focus: usize) -> bool {
        match item {
            Item::Launch => panes[focus].session.is_none(),
            Item::Restart => panes[focus].session.is_some(),
            // en reposo la misma tecla cierra el panel, salvo que sea el último
            Item::Kill => panes[focus].session.is_some() || panes.len() > 1,
            _ => true,
        }
    }

    /// Filas de arriba hacia abajo. `Item::Quit` va aparte, anclado al fondo.
    fn rows(&self, panes: &[Pane], focus: usize, tick: u64, width: u16) -> Vec<Row> {
        let dock = is_dock(width);
        let plain = |line: Line<'static>| Row { item: None, line, enabled: true };
        let mut rows = vec![plain(Line::from(""))];

        rows.push(Row {
            item: Some(Item::Logo),
            line: Line::from(Span::styled(
                "✻",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ))
            .centered(),
            enabled: true,
        });
        if !dock {
            rows.push(Row {
                item: Some(Item::Logo),
                line: Line::from(Span::styled(
                    "c l a w d p i l o t",
                    Style::default().add_modifier(Modifier::BOLD),
                ))
                .centered(),
                enabled: true,
            });
        }
        rows.push(plain(Line::from("")));

        if !dock {
            rows.push(plain(Line::from(Span::styled(" PANELES", Style::default().fg(DIM)))));
        }
        for (i, pane) in panes.iter().enumerate() {
            rows.push(Row {
                item: Some(Item::Pane(i)),
                line: pane_line(pane, i, i == focus, tick, dock, width),
                enabled: true,
            });
        }
        rows.push(Row {
            item: Some(Item::NewPane),
            line: if dock {
                Line::from(" +")
            } else {
                action_line("+", "nuevo panel", "^A n", width)
            },
            enabled: true,
        });
        rows.push(plain(Line::from("")));

        if !dock {
            rows.push(plain(Line::from(Span::styled(" ACCIONES", Style::default().fg(DIM)))));
        }
        for (item, icon, label, key) in ACTIONS {
            // `detener` se convierte en `cerrar panel` cuando no hay nada que detener
            let (icon, label) = match item {
                Item::Kill if panes[focus].session.is_none() => ("✕", "cerrar panel"),
                _ => (icon, label),
            };
            rows.push(Row {
                item: Some(item),
                line: if dock {
                    Line::from(format!(" {icon}"))
                } else {
                    action_line(icon, label, key, width)
                },
                enabled: self.enabled(item, panes, focus),
            });
        }
        rows
    }

    fn quit_row(&self, width: u16) -> Row {
        let dock = is_dock(width);
        Row {
            item: Some(Item::Quit),
            line: if dock {
                Line::from(" ⏻")
            } else {
                action_line("⏻", "salir", "^A q", width)
            },
            enabled: true,
        }
    }

    pub fn render(&self, area: Rect, frame: &mut Frame, panes: &[Pane], focus: usize, tick: u64) {
        let block = Block::new().borders(Borders::RIGHT).border_style(Style::default().fg(DIM));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let mut rows = self.rows(panes, focus, tick, inner.width);
        rows.truncate(inner.height.saturating_sub(2) as usize);
        for (i, row) in rows.iter().enumerate() {
            self.paint(frame, row, Rect { y: inner.y + i as u16, height: 1, ..inner });
        }
        if inner.height > 2 {
            let bottom = Rect { y: inner.bottom() - 1, height: 1, ..inner };
            self.paint(frame, &self.quit_row(inner.width), bottom);
        }
    }

    fn paint(&self, frame: &mut Frame, row: &Row, area: Rect) {
        let mut line = row.line.clone();
        if !row.enabled {
            line = line.style(Style::default().fg(DIM));
        }
        frame.render_widget(Paragraph::new(line), area);
        // el resalte va por encima: solo toca el fondo, así los colores se mantienen
        if row.item.is_some() && row.item == self.hover {
            frame.buffer_mut().set_style(area, Style::default().bg(HOVER));
        }
    }

    /// Qué hay bajo el puntero, si es que hay algo utilizable.
    pub fn hit(&self, area: Rect, x: u16, y: u16, panes: &[Pane], focus: usize) -> Option<Item> {
        let inner = Rect { width: area.width.saturating_sub(1), ..area };
        if x >= inner.right() || y < inner.y || y >= inner.bottom() {
            return None;
        }
        if inner.height > 2 && y == inner.bottom() - 1 {
            return Some(Item::Quit);
        }
        let rows = self.rows(panes, focus, 0, inner.width);
        let row = rows.get((y - inner.y) as usize)?;
        row.enabled.then_some(row.item).flatten()
    }
}

/// ` ▸1 claude   ● api` — o `` ●1`` cuando está plegado.
fn pane_line(
    pane: &Pane,
    i: usize,
    focused: bool,
    tick: u64,
    dock: bool,
    width: u16,
) -> Line<'static> {
    let (mark, style) = if focused {
        ("▸", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
    } else {
        (" ", Style::default())
    };
    let number = format!("{}", i + 1);
    let dot = status(pane, tick);
    if dock {
        return Line::from(vec![Span::styled(format!(" {number}"), style), dot]);
    }

    let here = pane.cwd.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    // el nombre del agente manda; la carpeta se queda con lo que sobre
    let name = clip(pane.agent_name(), 8);
    let left = format!(" {mark}{number} {name:<8}"); // ancho fijo: los estados hacen columna
    let room = (width as usize).saturating_sub(left.chars().count() + 2);
    Line::from(vec![
        Span::styled(left, style),
        Span::raw(" "),
        dot,
        Span::styled(format!(" {}", clip(&here, room)), Style::default().fg(DIM)),
    ])
}

fn status(pane: &Pane, tick: u64) -> Span<'static> {
    match (&pane.session, pane.is_busy()) {
        (Some(_), true) => Span::styled(
            SPIN[(tick / 4) as usize % SPIN.len()],
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        (Some(_), false) => Span::styled("●", Style::default().fg(Color::Green)),
        (None, _) => Span::styled("○", Style::default().fg(DIM)),
    }
}

/// ` ▶ lanzar      Enter` — icono a la izquierda, atajo pegado a la derecha.
fn action_line(icon: &str, label: &str, key: &str, width: u16) -> Line<'static> {
    let used = 2 + icon.chars().count() + 1 + label.chars().count() + key.chars().count();
    let gap = (width as usize).saturating_sub(used).max(1);
    Line::from(vec![
        Span::raw(format!(" {icon} {label}{:gap$}", "")),
        Span::styled(key.to_string(), Style::default().fg(DIM)),
    ])
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max.saturating_sub(1)).chain(['…']).collect()
}
