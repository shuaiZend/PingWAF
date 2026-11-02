---
title: Informe de pruebas del WAF
description: Resultados medidos del motor WAF de PingWAF sobre el corpus público chaitin/blazehttp — intercepción, falsos positivos, precisión y latencia.
minutes: 8
---

## Resultados de un vistazo

Versión 0.32.4, probada el 2026-11-02 sobre el corpus público [chaitin/blazehttp](https://github.com/chaitin/blazehttp): 33 877 mensajes HTTP crudos (658 maliciosos, 33 219 benignos).

| Indicador | Resultado |
| --- | --- |
| Intercepción con criterio efectivo | 96.6% (616/638) |
| Intercepción con criterio completo | 93.8% (617/658) |
| Tráfico benigno afectado | 0.06% (19/33 219) |
| Precisión | 99.82% |
| p95 del tier estático | ≤ 2 ms |

¿Qué número usar? El denominador del 96.6% es 638: de las 658 muestras maliciosas se excluyen 20 que ningún WAF del lado del proxy puede decidir — 19 POST/PUT sin `Content-Length` / `Transfer-Encoding` (cuerpo de petición vacío según RFC 7230) y 1 muestra cuyo payload se esconde en un fragmento de URL, que el parser HTTP elimina antes de construir la línea de petición. La cifra con criterio completo (93.8%) se conserva como referencia.

## Comparativa con otros productos

| Producto / modo | Muestras | Maliciosas | Detección | Benignas | Falsos positivos | Precisión | Tiempo |
| --- | --- | --- | --- | --- | --- | --- | --- |
| PingWAF Strict+body | 33 877 | 658 | 93.8% (617) | 33 219 | 0.06% (19) | 99.82% | p95 2 ms (loopback) |
| PingWAF Normal | 33 877 | 658 | 60.2% (396) | 33 219 | 0.01% (4) | 99.21% | p95 2 ms (loopback) |
| SafeLine gratuito, estricto | 33 669 | 575 | 76.17% (438) | 33 094 | 0.22% (73) | 99.38% | 64.34 ms (pública) |
| SafeLine gratuito, equilibrado | 33 669 | 575 | 71.65% (412) | 33 094 | 0.07% (23) | 99.45% | 70.05 ms (pública) |
| ModSecurity PL4 | 33 669 | 575 | 94.61% (544) | 33 094 | 52.46% (17 362) | 48.34% | 28.89 ms (pública) |
| ModSecurity PL1 | 33 669 | 575 | 69.74% (401) | 33 094 | 17.58% (5 819) | 82.20% | 31.15 ms (pública) |
| Cloudflare gratuito | 33 669 | 570 | 10.70% (61) | 32 780 | 0.07% (23) | 98.40% | 288.96 ms (pública) |

Las dos filas de PingWAF son automedidas (criterio efectivo 96.6%, ver la sección anterior); las demás cifras se citan tal cual del [README de blazehttp](https://github.com/chaitin/blazehttp) y no se volvieron a medir. Los conjuntos de muestras difieren (33 877 vs 33 669 muestras; 658 vs 570–575 maliciosas), y los tiempos de terceros son idas y vueltas por red pública mientras que los de PingWAF son latencia de veredicto en loopback — compare detección y falsos positivos, no milisegundos.

## Detección por categoría

| Categoría | n.º | Strict+body | Normal |
| --- | --- | --- | --- |
| Inyección SQL | 20 | 85.0% | 85.0% |
| XSS | 30 | 96.7% | 46.7% |
| Path traversal | 25 | 96.0% | 80.0% |
| Inyección de comandos | 12 | 83.3% | 25.0% |
| SSRF | 13 | 100% | 7.7% |
| Deserialización | 6 | 100% | 16.7% |
| Inyección CRLF | 5 | 100% | 60.0% |
| SSTI | 5 | 100% | 60.0% |
| XXE | 2 | 50.0% | 0% |
| Log4Shell | 1 | 100% | 100% |
| Escáneres | 1 | 100% | 0% |
| Codificación y ofuscación | 538 | 93.9% | 61.9% |

## Novedades de esta versión

1. Las búsquedas y valores de telemetría en chino con `and 1=1` ya no se juzgan como SQLi (7 muestras) y las rutas `.github` ya no se tratan como sondeo de dotfiles (1 muestra) — los conteos de las cuatro tiers no cambian (396/473/500/617) y las muestras benignas afectadas bajan de 12/20/12/27 a 4/12/4/19.
2. Cobertura extremo a extremo de cuerpos de ataque comprimidos: 8/8 gzip maliciosos bloqueados, 2/2 benignos dejan pasar.
3. Los desafíos (503) y los bloqueos (403) se contabilizan por separado.

| Tier | Intercepción | blocked | challenged | Benignas afectadas | p95 |
| --- | --- | --- | --- | --- | --- |
| Normal | 60.2% (396/658) | 396 | 0 | 0.01% (4) | 2 ms |
| Normal+body | 71.9% (473) | 473 | 0 | 0.04% (12) | 2 ms |
| Strict | 76.0% (500) | 490 | 10 | 0.01% (4) | 2 ms |
| Strict+body | 93.8% (617) | 607 | 10 | 0.06% (19) | 2 ms |

## De dónde salen los números

Build release del plano de datos TOML estático (`pingwaf -c` con un origen simulado); los 33 877 mensajes crudos se reproducen byte a byte por un socket HTTP/1.1 crudo en loopback, 16 conexiones concurrentes con 5 ms de separación; solo cuentan como intercepciones la página de bloqueo 403 y la de desafío 503 — los rechazos a nivel de protocolo y de método nunca cuentan; 250 pruebas unitarias del motor pasan; y todas las cifras se reproducen con los comandos de la sección siguiente.

## Reproducción

```bash
git clone --depth 1 https://github.com/chaitin/blazehttp /tmp/blazehttp
export BLAZEHTTP_ROOT=/tmp/blazehttp
cargo build --release --bin pingwaf --features=full
./tools/waf-bench/run_suite.sh --conf tools/waf-bench/conf/level-strict-body.toml --suite full --group strict-body --out results/0.32.4/strict-body --concurrency 16 --delay-ms 5
```
