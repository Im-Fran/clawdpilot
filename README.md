<div align="center">

# ✻ clawdpilot

**Pilotea varios agentes de Claude Code a la vez, en una sola pantalla.**

Una TUI en Rust que abre terminales reales — un `claude` interactivo dentro de cada
una — y te deja saltar entre ellos como si fueran las estaciones de un war room.

[![CI](https://github.com/Im-Fran/clawdpilot/actions/workflows/ci.yml/badge.svg?branch=dev)](https://github.com/Im-Fran/clawdpilot/actions/workflows/ci.yml)
[![Licencia: GPL v3](https://img.shields.io/badge/licencia-GPL--3.0-blue.svg)](LICENSE)
[![Rust 1.85+](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org)

</div>

---

## 📖 Qué es

Trabajar con varios agentes en paralelo hoy significa varias ventanas, varias pestañas de tmux
y ningún sitio desde donde verlo todo. `clawdpilot` pone tus sesiones de Claude Code en una
rejilla y te da un atajo para moverte entre ellas. Arranca con cuatro y añades o cierras
paneles sobre la marcha; la rejilla se reacomoda sola.

No es un envoltorio ni una reimplementación de la interfaz de Claude: cada panel es un
**pseudo-terminal de verdad** con el binario `claude` corriendo dentro. Ves su TUI tal cual —
colores, el spinner, los prompts de permisos, los `/comandos`, el diálogo de "trust this folder".
Todo lo que funciona en tu terminal funciona dentro de un panel.

Cada agente puede trabajar en un directorio distinto, así que puedes tener uno refactorizando la
API, otro escribiendo tests, otro leyendo un repo ajeno y el cuarto de reserva.

---

## ✨ Características

- **PTYs reales** — la TUI completa de Claude Code en cada panel, sin recortes
- **Paneles a demanda** — `^A n` añade uno, `^A x` cierra el enfocado; la rejilla se recalcula
  sola (2×2, 3×2, 3×3…) y se niega a crear paneles que quedarían ilegibles
- **Un directorio por agente** — por argumento al arrancar o cambiándolo en caliente con `^A c`
- **Passthrough total de teclado** — lo que escribes llega al agente enfocado byte a byte,
  incluidas flechas, `Esc`, `Tab` y combinaciones con `Ctrl`/`Alt`
- **Pegado con bracketed paste** — pegar un prompt largo no dispara autocompletados raros
- **Zoom** — expande el panel enfocado a pantalla completa cuando necesitas leer de verdad
- **Reiniciar y matar** agentes sin salir de la aplicación
- **Redimensionado en vivo** — al cambiar el tamaño de la ventana, cada PTY se reajusta solo
- **Arranque perezoso** — los paneles empiezan en reposo; lanzas los agentes que quieras

---

## 🛠 Stack

| Pieza | Tecnología |
|-------|-----------|
| Lenguaje | Rust 2024 |
| Interfaz | [`ratatui`](https://ratatui.rs) + crossterm |
| Terminales | [`portable-pty`](https://docs.rs/portable-pty) |
| Emulación VT | [`vt100`](https://docs.rs/vt100) |
| Errores | [`anyhow`](https://docs.rs/anyhow) |

---

## 📋 Requisitos

- **Rust** >= 1.85 (edición 2024)
- **Claude Code** instalado y accesible como `claude` en el `PATH`
- Un terminal con soporte de 256 colores

---

## 🚀 Uso

```bash
cargo run --release
```

Arranca con cuatro agentes sobre el directorio actual. Para repartirlos por proyecto:

```bash
cargo run --release -- ~/proyectos/api ~/proyectos/web ~/proyectos/docs
```

Cada ruta abre su panel. Si pasas más de cuatro, la rejilla crece; si pasas menos, los paneles
restantes heredan la primera ruta. Cada ruta debe ser un directorio existente — si no, la
aplicación se niega a arrancar en vez de dejarte un panel roto.

Con la sesión ya abierta, `^A n` añade paneles y `^A x` los cierra.

Para instalarlo en el sistema:

```bash
cargo install --path .
clawdpilot ~/proyectos/api ~/proyectos/web
```

---

## ⌨️ Controles

Con un agente vivo, **todas** las teclas le pertenecen a él. Por eso los comandos de la
aplicación viven detrás de un prefijo estilo tmux: `Ctrl+A`.

| Tecla | Acción |
|-------|--------|
| `Enter` | Lanza el agente del panel enfocado (solo si está en reposo) |
| `Tab` | Siguiente panel (solo si el actual está en reposo) |
| `Ctrl+A` | Entra en modo comando |

Ya dentro del modo comando:

| Tecla | Acción |
|-------|--------|
| `1` … `9` | Enfocar ese panel |
| `Tab` | Siguiente panel |
| `n` | Nuevo panel, heredando la carpeta del enfocado |
| `z` | Zoom del panel enfocado a pantalla completa |
| `r` | Reiniciar el agente del panel |
| `x` | Matar el agente; sobre un panel ya en reposo, cierra el panel |
| `c` | Cambiar el directorio de trabajo del panel |
| `q` | Salir (mata los cuatro agentes) |
| `Ctrl+A` | Enviar un `Ctrl+A` literal al agente |

La barra inferior siempre muestra las teclas disponibles según dónde estés, y avisa en amarillo
cuando una acción no se puede hacer — por ejemplo si ya no cabe otro panel.

Siempre queda al menos un panel abierto: `^A x` sobre el último no lo cierra.

---

## ⚙️ Variables de entorno

| Variable | Por defecto | Descripción |
|----------|-------------|-------------|
| `CLAWDPILOT_CLAUDE` | `claude` | Binario que se lanza en cada panel. Útil si tienes varias versiones instaladas o para pruebas. |

---

## 🔍 Cómo funciona

```
  teclado  ──►  encode_key  ──►  PTY master  ──►  claude
                                                    │
  pantalla ◄──  ratatui     ◄──  vt100 Parser  ◄────┘
```

La rejilla es la más cuadrada que quepa: para `n` paneles se usan `ceil(sqrt(n))` columnas, y la
última fila reparte a lo ancho los que le quedan. Antes de añadir un panel se comprueba que
todos seguirían midiendo al menos 20×6; si no, la acción se rechaza en vez de dejarte una
cuadrícula ilegible.

`portable-pty` lanza `claude` en un pseudo-terminal del tamaño exacto del panel. Un hilo lector
vuelca los bytes en un parser `vt100` compartido, que mantiene la pantalla del agente como una
rejilla de celdas con sus atributos. En cada fotograma, el hilo de dibujo copia esa rejilla al
búfer de `ratatui`, celda a celda, y coloca el cursor real del terminal donde lo tenga el panel
enfocado.

Al revés, cada tecla se traduce a los bytes que un `xterm` enviaría — `\r`, `0x7f`, `\x1b[A`,
`Ctrl+letra` como byte de control — y se escribe en el maestro del PTY. El agente no distingue
`clawdpilot` de un terminal normal.

Son dos archivos: `src/pane.rs` (PTY, vt100 y el render de la rejilla) y `src/main.rs`
(distribución, modos de teclado y bucle de eventos).

---

## 🧪 Desarrollo

```bash
cargo test              # rejilla, layout, modo comando y el camino PTY → búfer
cargo clippy --all-targets
```

Hay además una prueba marcada como `ignored` que lanza el binario `claude` real y comprueba que
su interfaz llega hasta el búfer de `ratatui`:

```bash
cargo test -- --ignored --nocapture
```

---

## 🤝 Contribuir

Las contribuciones son bienvenidas. Lee [CONTRIBUTING.md](CONTRIBUTING.md) para el flujo completo
y [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) para las reglas de convivencia.

En corto:

1. Crea una rama desde `dev`: `git checkout -b feat/lo-tuyo`
2. Deja `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings` y `cargo test` en verde
3. Commitea siguiendo [Conventional Commits](https://www.conventionalcommits.org/es/)
4. Abre un PR contra `dev`

¿Encontraste un problema de seguridad? No abras un issue: sigue [SECURITY.md](SECURITY.md).

---

## 📄 Licencia

[GNU General Public License v3.0](LICENSE) — puedes usar, modificar y redistribuir el proyecto,
siempre que los trabajos derivados se publiquen bajo la misma licencia.

---

<div align="center">
Hecho con ☕ por <a href="https://franciscosolis.cl">Fran</a>
</div>
