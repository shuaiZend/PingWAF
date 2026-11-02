---
title: Informe de pruebas del WAF
description: Efectividad medida del motor WAF de PingWAF sobre el corpus público chaitin/blazehttp — intercepción, falsos positivos, latencia, regresión y determinismo.
minutes: 10
---

## Resumen del veredicto

Versión 0.32.4, probada el 2026-11-02 sobre el dataset público [chaitin/blazehttp](https://github.com/chaitin/blazehttp): 33 877 mensajes HTTP crudos (658 maliciosos, 33 219 benignos). Todas las cifras siguientes usan el criterio estricto — solo cuenta como intercepción una página de bloqueo 403 o una página de desafío 503.

| Tier | Intercepción | blocked | challenged | Benignos afectados |
| --- | --- | --- | --- | --- |
| Normal | 60.2% (396/658) | 396 | 0 | 0.01% (4/33 219) |
| Normal + body | 71.9% (473/658) | 473 | 0 | 0.04% (12) |
| Strict | 76.0% (500/658) | 490 | 10 | 0.01% (4) |
| Strict + body | 93.8% (617/658) | 607 | 10 | 0.06% (19) |

Las tasas de intercepción no cambian respecto a la ronda anterior; lo que varía es el número de muestras benignas afectadas. Normal es el tier por defecto y no inspecciona el cuerpo de las peticiones. El tier máximo (Strict + inspección de cuerpo) bloquea el 93.8% del corpus malicioso público con un 0.06% de falsos positivos.

## Método de prueba

- **Plano de datos estático.** El motor arranca desde una configuración TOML (`pingwaf -c`) con un origen simulado; no interviene ningún comportamiento de agente.
- **Replay byte a byte por socket crudo.** Las 33 877 muestras se reproducen byte a byte por un socket HTTP/1.1 crudo, conservando el orden de cabeceras, trucos de codificación y entradas malformadas.
- **Criterio estricto.** `blocked` es una página de bloqueo 403, `challenged` es una página de desafío 503 y `intercepted` es la suma de ambos (comparable con las cifras históricas). Solo cuentan estos veredictos del WAF; los rechazos a nivel de protocolo y los errores nunca cuentan como intercepciones.
- **Criterio efectivo.** Hay 20 muestras maliciosas que ningún inspector en el lado del proxy puede decidir (POST/PUT sin cuerpo, rechazos de protocolo, un fragmento de URL). Excluyéndolas, la cifra efectiva del tier recomendado es **616/638 = 96.6%**.

## Intercepción por categoría

| Category | n | Normal | Strict+body |
| --- | --- | --- | --- |
| sqli | 20 | 85.0% (17) | 85.0% (17) |
| xss | 30 | 46.7% (14) | 96.7% (29) |
| lfi | 25 | 80.0% (20) | 96.0% (24) |
| rce | 12 | 25.0% (3) | 83.3% (10) |
| ssrf | 13 | 7.7% (1) | 100% (13) |
| other | 538 | 61.9% (333) | 93.9% (505) |

La distancia entre Normal y Strict+body la sostienen las familias cuyo payload solo existe en el cuerpo de la petición (ssrf 7.7% → 100%) y las variantes codificadas u ofuscadas (`other` 61.9% → 93.9%). La inyección SQL ya queda casi cubierta en el tier por defecto.

En la suite de payloads comprimidos (`compressed`), Strict+body intercepta las 8 muestras maliciosas y deja pasar las 2 benignas (2/2); Normal+body intercepta 7/8 y se le escapa el payload `$(curl … | sh)` comprimido con gzip.

## Regresión y determinismo

- Suite `triage`: 20 muestras maliciosas actualmente no detectadas fijadas y reproducidas — veredictos idénticos a la ejecución completa, muestra por muestra: 19 pasan, 1 responde con HTTP 405.
- Suite `fp-pin`: 27 muestras benignas actualmente bloqueadas fijadas y reproducidas — 17 siguen bloqueadas, 2 con desafío y 8 ahora pasan (los falsos positivos eliminados en esta ronda).
- `spot-advanced` (ruta advanced_mode) frente a Strict+body sobre las 33 877 muestras: **0 diferencias**, bit a bit.
- Pruebas unitarias: **250** en `pingwaf-waf`, todas en verde durante la ejecución.

## Rendimiento y limitaciones

Con build release y en loopback, la latencia del tier estático se mantiene en **p95 1–2 ms**. No se declara throughput: el harness de replay es un reproductor de fidelidad, no un generador de carga. La prueba cubre solo el plano de datos TOML estático — el comportamiento del agente (envío de logs, cookies de paso, escalado a bloqueo) queda fuera del alcance. Tampoco se ejercita tráfico TLS, HTTP/2 ni HTTP/3.

## Comparativa con otros WAF

El mismo corpus público también se ha usado para evaluar otros productos. Las cifras de terceros de la tabla siguiente se citan tal cual del [README de blazehttp](https://github.com/chaitin/blazehttp) (criterio de 33 669 muestras); las cifras de PingWAF provienen de la prueba descrita arriba.

| Producto / modo | Muestras | Maliciosas | Detección | Benignas | Falsos positivos | Precisión | Tiempo |
| --- | --- | --- | --- | --- | --- | --- | --- |
| PingWAF Strict+body | 33 877 | 658 | 93.8% (617) | 33 219 | 0.06% (19) | 99.82% | p95 2 ms (loopback) |
| PingWAF Normal | 33 877 | 658 | 60.2% (396) | 33 219 | 0.01% (4) | 99.21% | p95 2 ms (loopback) |
| SafeLine gratuito, estricto | 33 669 | 575 | 76.17% (438) | 33 094 | 0.22% (73) | 99.38% | 64.34 ms (pública) |
| SafeLine gratuito, equilibrado | 33 669 | 575 | 71.65% (412) | 33 094 | 0.07% (23) | 99.45% | 70.05 ms (pública) |
| ModSecurity PL4 | 33 669 | 575 | 94.61% (544) | 33 094 | 52.46% (17 362) | 48.34% | 28.89 ms (pública) |
| ModSecurity PL1 | 33 669 | 575 | 69.74% (401) | 33 094 | 17.58% (5 819) | 82.20% | 31.15 ms (pública) |
| Cloudflare gratuito | 33 669 | 570 | 10.70% (61) | 32 780 | 0.07% (23) | 98.40% | 288.96 ms (pública) |

Cómo leer esta tabla:

1. Las instantáneas de muestras difieren (oficial: 33 669 muestras con 570–575 maliciosas; PingWAF: 33 877 con 658), así que las tasas de detección no comparten el mismo denominador.
2. El tiempo se mide de forma distinta: las cifras de red pública incluyen el RTT, mientras que las de PingWAF son la latencia de veredicto en loopback — ninguna sirve como comparación de throughput.
3. Los números de terceros se citan tal cual y no se volvieron a medir en esta ronda; la tabla solo describe la posición relativa de cada motor sobre el mismo corpus público.

## Reproducción e informe completo

El informe completo, con la atribución de falsos positivos por regla y el análisis de artefactos, vive en el repositorio:

- [Informe completo de pruebas (EN)](https://github.com/shuaiZend/PingWAF/blob/main/docs/waf-test-report.md)
- [Plan de pruebas y definición de criterios](https://github.com/shuaiZend/PingWAF/blob/main/docs/waf-test-plan.md)

La metodología completa y la definición de criterios están en el
[plan de pruebas](https://github.com/shuaiZend/PingWAF/blob/main/docs/waf-test-plan.md).
