# Política de seguridad

pptalk es una versión de desarrollo y no ha recibido una auditoría de seguridad
independiente. No debe usarse todavía en situaciones donde una vulnerabilidad
pueda poner a alguien en peligro.

## Reportar una vulnerabilidad

Usa de forma privada **GitHub Security Advisories** en este repositorio. Incluye:

- versión o commit afectado;
- sistema operativo;
- pasos mínimos para reproducirlo;
- impacto esperado;
- cualquier mitigación conocida.

No abras un issue público antes de que exista una corrección coordinada.

El [modelo de amenazas](docs/threat-model.md) explica qué intenta proteger el
proyecto, qué metadatos permanecen visibles y cuáles son las limitaciones
locales aceptadas en esta fase.

Los cambios se comprueban con tests de Rust, Clippy y la base de avisos RustSec.
