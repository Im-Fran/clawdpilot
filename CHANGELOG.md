# Changelog

Todos los cambios notables de este proyecto se documentan en este archivo.

El formato sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/) y el proyecto se adhiere
a [Versionado Semántico](https://semver.org/lang/es/).

## [No publicado]

### Añadido

- Licencia GPL-3.0
- `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md` y `SECURITY.md`
- Integración continua en GitHub Actions: `cargo fmt --check`, `cargo clippy -D warnings` y
  `cargo test` en Linux y macOS
- Metadatos del paquete en `Cargo.toml` (descripción, licencia, repositorio, keywords)

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
