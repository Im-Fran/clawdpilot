# Changelog

Todos los cambios notables de este proyecto se documentan en este archivo.

El formato sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/) y el proyecto se adhiere
a [Versionado Semántico](https://semver.org/lang/es/).

## [No publicado]

### Añadido

- Soporte para otras IAs de terminal además de Claude Code: `^A a` abre una lista para elegir
  el agente del panel entre `claude`, `codex`, `agy` (Antigravity) y `aider`, y el título del
  panel muestra cuál corre
- Ratón: clic para enfocar un panel, clic en su barra de título para abrir la lista de agentes,
  y clic o rueda sobre una fila para elegirla
- Los clics y la rueda dentro del panel enfocado se reenvían al agente como reportes SGR, solo
  si su interfaz ha pedido el ratón
- `CLAWDPILOT_AGENTS`: lista de agentes separada por comas, cada uno con sus argumentos
- Número de paneles variable: `^A n` añade uno y `^A x` cierra el enfocado. La rejilla se
  recalcula sola (2×2, 3×2, 3×3…) y la última fila reparte a lo ancho los paneles que le quedan
- Se rechaza crear un panel que dejaría la rejilla por debajo de 20×6 por celda, con aviso en
  el footer en lugar de fallar en silencio
- Licencia GPL-3.0
- `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md` y `SECURITY.md`
- Integración continua en GitHub Actions: `cargo fmt --check`, `cargo clippy -D warnings` y
  `cargo test` en Linux y macOS
- Metadatos del paquete en `Cargo.toml` (descripción, licencia, repositorio, keywords)

### Cambiado

- `^A x` sobre un panel ya en reposo ahora lo cierra, en vez de no hacer nada
- Enfoque directo con `^A 1`…`^A 9` (antes `^A 1`…`^A 4`)
- Se acabó el tope de cuatro directorios por línea de comandos: se abre un panel por ruta
- Si el agente no se puede lanzar, se avisa en el footer en vez de cerrar la aplicación

### Eliminado

- `CLAWDPILOT_CLAUDE`, reemplazada por `CLAWDPILOT_AGENTS`

## [0.1.0] - 2026-08-30

Primera versión. No publicada como release: se compila desde el repositorio.

### Añadido

- TUI en rejilla 2×2 con cuatro pseudo-terminales reales, cada uno ejecutando `claude`
- Un directorio de trabajo por agente, indicado al arrancar o cambiado en caliente con `^A c`
- Passthrough completo de teclado hacia el agente enfocado, incluidas flechas, `Esc`, `Tab` y
  combinaciones `Ctrl`/`Alt`
- Pegado con *bracketed paste*
- Modo comando tras el prefijo `Ctrl+A`: enfocar panel, zoom, reiniciar, matar, cambiar directorio
  y salir
- Redimensionado en vivo de cada PTY al cambiar el tamaño de la ventana
- Arranque perezoso: los paneles empiezan en reposo y se lanzan bajo demanda
- Variable de entorno `CLAWDPILOT_CLAUDE` para elegir el binario a ejecutar

[No publicado]: https://github.com/Im-Fran/clawdpilot/compare/v0.1.0...dev
[0.1.0]: https://github.com/Im-Fran/clawdpilot/releases/tag/v0.1.0
