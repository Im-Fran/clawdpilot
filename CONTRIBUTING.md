# Contribuir a clawdpilot

Gracias por pasarte. Esto es un proyecto pequeño: dos archivos de código y una idea concreta.
Las contribuciones que mejor encajan son las que lo mantienen así.

## Antes de escribir código

- Para un **bug**: abre un issue con el terminal que usas, tu sistema operativo y los pasos para
  reproducirlo. Si el fallo es visual, un `asciinema` o una captura ayudan mucho.
- Para una **feature**: abre un issue primero y discutámosla. Prefiero rechazar una idea en un
  comentario que rechazar un PR de 400 líneas que alguien ya escribió.

## Flujo

1. Haz fork y crea una rama desde `dev`: `git checkout -b feat/lo-tuyo`
2. Escribe el cambio y, si toca lógica, el test que falla sin él.
3. Deja el árbol en verde:

   ```bash
   cargo fmt --all
   cargo clippy --all-targets -- -D warnings
   cargo test
   ```

4. Commitea siguiendo [Conventional Commits](https://www.conventionalcommits.org/es/): `feat:`,
   `fix:`, `docs:`, `refactor:`, `test:`, `chore:`.
5. Abre el PR contra `dev` (es la rama principal del repo, no hay `main`).

## Qué espera el CI

El workflow corre `cargo fmt --check`, `cargo clippy -D warnings` y `cargo test` en Linux y macOS.
Si pasa en tu máquina, pasa allí.

Los tests marcados `#[ignore]` lanzan el binario `claude` de verdad y no corren en CI. Si tocas el
camino PTY → búfer, córrelos a mano:

```bash
cargo test -- --ignored --nocapture
```

## Estilo

- Rust idiomático y `cargo fmt`. El único ajuste es `use_small_heuristics = "Max"` en
  `rustfmt.toml`, para que las structs y llamadas cortas quepan en una línea. No añadas más.
- Sin dependencias nuevas salvo que resuelvan algo que no se puede hacer en unas pocas líneas.
  El proyecto tiene cuatro y la idea es que siga teniendo cuatro.
- Comentarios donde el *por qué* no sea evidente, no donde el *qué* ya lo dice el código.

## Licencia

Al contribuir aceptas que tu código se publique bajo la [GPL-3.0](LICENSE), igual que el resto
del proyecto.
