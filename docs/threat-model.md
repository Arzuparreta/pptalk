# Seguridad y límites

pptalk está diseñado para reducir la confianza en servidores, pero no vuelve
invisible al usuario ni protege un ordenador ya comprometido.

## Qué protege

- El contenido de mensajes y archivos frente a relays, buzones, operadores y
  observadores de red.
- El contenido de llamadas durante el transporte.
- Los cambios de identidad y dispositivo frente a falsificaciones o forks.
- El historial local mediante una base SQLCipher.
- Las invitaciones frente a modificaciones durante su vigencia.

## Qué sigue siendo visible

Un relay u observador puede inferir cuándo hay dispositivos conectados, qué
volumen de tráfico generan y durante cuánto tiempo. Un buzón conoce tamaños,
caducidades y el uso de cada capacidad. El padding reduce filtraciones de tamaño,
pero pptalk no es una red de anonimato.

Los nombres visibles de los contactos no están registrados globalmente.

## Qué debe ser de confianza

El sistema operativo, el proceso de pptalk y los dispositivos de entrada y
salida deben estar limpios. Malware con acceso a una sesión desbloqueada puede
leer mensajes, capturar pantalla o usar el micrófono.

Una invitación demuestra posesión del enlace, no la identidad civil de quien lo
envió. La interfaz actual aún no muestra una comparación de huellas: cuando esa
verificación sea necesaria, no confíes únicamente en el nombre visible ni uses
esta versión para una relación de alto riesgo.

## Pérdida o robo de un dispositivo

Otro dispositivo autorizado puede revocarlo. Los peers dejan de aceptar su
estado antiguo y los grupos controlados cambian de época MLS.

La revocación no borra los archivos que el equipo robado ya tuviera ni vence a
malware que extrajo claves antes de ser revocado.

Si pierdes todos los dispositivos, no hay recuperación central. Esa es la
contrapartida de no mantener una autoridad con capacidad de restablecer tu
identidad.

## Protección local actual

La conversación está en SQLCipher, pero la clave de esa base reside actualmente
junto al perfil del dispositivo, protegido con permisos de usuario. Para esta
versión se asume cifrado completo de disco y una sesión de escritorio bloqueada.

Mover las semillas y claves al almacén seguro del sistema operativo sigue siendo
necesario antes de considerar el cliente endurecido.

## Fuera de alcance actualmente

- anonimato de red;
- descubrimiento público de personas;
- comunidades y bots;
- grabación o transcripción;
- acceso desde navegador;
- copia de seguridad en la nube;
- overlays para juegos.

## Estado de auditoría

No ha habido una auditoría de seguridad independiente. No uses esta versión en
situaciones donde una vulnerabilidad pueda poner a alguien en peligro. Para
reportar un fallo consulta [SECURITY.md](../SECURITY.md).
