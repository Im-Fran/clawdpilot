# Política de seguridad

## Versiones soportadas

El proyecto se distribuye compilándolo desde el repositorio. Solo se da soporte a la punta de la
rama `dev`.

| Versión | Soportada |
|---------|-----------|
| `dev` (HEAD) | ✅ |
| Cualquier commit anterior | ❌ |

## Reportar una vulnerabilidad

**No abras un issue público.**

Usa el reporte privado de GitHub: pestaña
[**Security → Report a vulnerability**](https://github.com/Im-Fran/clawdpilot/security/advisories/new).
Solo lo vemos el mantenedor y tú.

Incluye, si puedes:

- Qué versión (commit) estabas corriendo
- Sistema operativo y terminal
- Pasos para reproducirlo
- El impacto que le ves

Respondo en un plazo razonable — esto es un proyecto personal, no esperes un SLA. Si el reporte es
válido, coordino contigo la publicación del arreglo antes de hacerlo público.

## Superficie de ataque

Para calibrar qué cuenta como vulnerabilidad aquí: `clawdpilot` lanza el binario `claude` dentro de
pseudo-terminales locales y le reenvía tus pulsaciones. No abre puertos, no habla por red y no
guarda credenciales.

Lo que sí es relevante reportar:

- Escapes de terminal que permitan que la salida de un agente ejecute algo fuera de su panel
- Cualquier forma de que un panel lea o escriba en el PTY de otro
- Ejecución de un binario distinto al esperado vía `CLAWDPILOT_CLAUDE` o resolución del `PATH`

Los fallos de seguridad de Claude Code en sí van a
[anthropic.com/security](https://www.anthropic.com/security), no aquí.
