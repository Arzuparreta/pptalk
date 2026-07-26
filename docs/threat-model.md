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
envió. Compara la huella completa que muestra la conversación por otro canal.
Marcarla como verificada registra tu comprobación local; no crea una autoridad
central ni certifica por sí solo quién controla el otro dispositivo.

## Pérdida o robo de un dispositivo

Otro dispositivo autorizado puede revocarlo. Los peers dejan de aceptar su
estado antiguo y los grupos controlados cambian de época MLS.

La revocación no borra los archivos que el equipo robado ya tuviera ni vence a
malware que extrajo claves antes de ser revocado.

Si pierdes todos los dispositivos, no hay recuperación central. Una copia local
cifrada permite restaurar la identidad, pero quien obtenga el archivo y su frase
también puede controlarla.

## Protección local actual

La conversación está en SQLCipher. De forma predeterminada, su clave reside junto
al perfil para mantener compatibilidad. **Protección local** la mueve al almacén
seguro del sistema y escribe ceros en el perfil solo después de comprobar que
puede recuperarla. Las demás semillas del dispositivo siguen requiriendo cifrado
completo de disco y una sesión de escritorio bloqueada.

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
