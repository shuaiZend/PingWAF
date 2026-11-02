---
title: Rapport de tests WAF
description: Résultats mesurés du moteur WAF de PingWAF sur le corpus public chaitin/blazehttp — interception, faux positifs, précision et latence.
minutes: 8
---

## Résultats en bref

Version 0.32.4, testée le 2026-11-02 sur le corpus public [chaitin/blazehttp](https://github.com/chaitin/blazehttp) : 33 877 messages HTTP bruts (658 malveillants, 33 219 bénins).

| Indicateur | Résultat |
| --- | --- |
| Interception au calibre effectif | 96.6% (616/638) |
| Interception au calibre complet | 93.8% (617/658) |
| Trafic bénin affecté | 0.06% (19/33 219) |
| Précision | 99.82% |
| p95 du tier statique | ≤ 2 ms |

Quel chiffre utiliser ? Le dénominateur des 96.6% est 638 : sur les 658 échantillons malveillants, 20 sont exclus car aucun WAF côté proxy ne peut les trancher — 19 POST/PUT sans `Content-Length` / `Transfer-Encoding` (corps de requête vide au sens du RFC 7230) et 1 échantillon dont le payload se cache dans un fragment d'URL, que le parseur HTTP retire avant de construire la ligne de requête. Le chiffre au calibre complet (93.8%) est conservé pour comparaison.

## Comparaison avec d'autres produits

| Produit / mode | Échantillons | Malveillants | Détection | Bénins | Faux positifs | Précision | Temps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| PingWAF Strict+body | 33 877 | 658 | 93.8% (617) | 33 219 | 0.06% (19) | 99.82% | p95 2 ms (loopback) |
| PingWAF Normal | 33 877 | 658 | 60.2% (396) | 33 219 | 0.01% (4) | 99.21% | p95 2 ms (loopback) |
| SafeLine gratuit, strict | 33 669 | 575 | 76.17% (438) | 33 094 | 0.22% (73) | 99.38% | 64.34 ms (public) |
| SafeLine gratuit, équilibré | 33 669 | 575 | 71.65% (412) | 33 094 | 0.07% (23) | 99.45% | 70.05 ms (public) |
| ModSecurity PL4 | 33 669 | 575 | 94.61% (544) | 33 094 | 52.46% (17 362) | 48.34% | 28.89 ms (public) |
| ModSecurity PL1 | 33 669 | 575 | 69.74% (401) | 33 094 | 17.58% (5 819) | 82.20% | 31.15 ms (public) |
| Cloudflare gratuit | 33 669 | 570 | 10.70% (61) | 32 780 | 0.07% (23) | 98.40% | 288.96 ms (public) |

Les deux lignes PingWAF sont automesurées (calibre effectif 96.6%, voir la section ci-dessus) ; les autres chiffres sont cités tels quels depuis le [README de blazehttp](https://github.com/chaitin/blazehttp), sans retest. Les jeux d'échantillons diffèrent (33 877 vs 33 669 échantillons ; 658 vs 570–575 malveillants), et les temps des tiers sont des allers-retours en réseau public tandis que ceux de PingWAF sont une latence de verdict en loopback — comparez la détection et les faux positifs, pas les millisecondes.

## Détection par catégorie

| Catégorie | n | Strict+body | Normal |
| --- | --- | --- | --- |
| Injection SQL | 20 | 85.0% | 85.0% |
| XSS | 30 | 96.7% | 46.7% |
| Path traversal | 25 | 96.0% | 80.0% |
| Injection de commandes | 12 | 83.3% | 25.0% |
| SSRF | 13 | 100% | 7.7% |
| Désérialisation | 6 | 100% | 16.7% |
| Injection CRLF | 5 | 100% | 60.0% |
| SSTI | 5 | 100% | 60.0% |
| XXE | 2 | 50.0% | 0% |
| Log4Shell | 1 | 100% | 100% |
| Scanners | 1 | 100% | 0% |
| Encodage et obfuscation | 538 | 93.9% | 61.9% |

## Améliorations de cette version

1. Les recherches et valeurs de télémétrie en chinois contenant `and 1=1` ne sont plus jugées SQLi (7 échantillons) et les chemins `.github` ne sont plus traités comme du sondage de dotfiles (1 échantillon) — les comptages des quatre tiers restent inchangés (396/473/500/617) et les échantillons bénins affectés passent de 12/20/12/27 à 4/12/4/19.
2. Couverture de bout en bout des corps d'attaque compressés : 8/8 gzip malveillants bloqués, 2/2 bénins laissés passer.
3. Les défis (503) et les blocages (403) sont comptabilisés séparément.

| Tier | Interception | blocked | challenged | Bénins affectés | p95 |
| --- | --- | --- | --- | --- | --- |
| Normal | 60.2% (396/658) | 396 | 0 | 0.01% (4) | 2 ms |
| Normal+body | 71.9% (473) | 473 | 0 | 0.04% (12) | 2 ms |
| Strict | 76.0% (500) | 490 | 10 | 0.01% (4) | 2 ms |
| Strict+body | 93.8% (617) | 607 | 10 | 0.06% (19) | 2 ms |

## D'où viennent les chiffres

Build release du plan de données TOML statique (`pingwaf -c` avec une origine simulée) ; les 33 877 messages bruts sont rejoués octet par octet via un socket HTTP/1.1 brut en loopback, 16 connexions concurrentes espacées de 5 ms ; seules la page de blocage 403 et la page de défi 503 comptent comme interceptions — les rejets au niveau du protocole ou de la méthode ne comptent jamais ; 250 tests unitaires du moteur passent ; et chaque chiffre se reproduit avec les commandes de la section suivante.

## Reproduction

```bash
git clone --depth 1 https://github.com/chaitin/blazehttp /tmp/blazehttp
export BLAZEHTTP_ROOT=/tmp/blazehttp
cargo build --release --bin pingwaf --features=full
./tools/waf-bench/run_suite.sh --conf tools/waf-bench/conf/level-strict-body.toml --suite full --group strict-body --out results/0.32.4/strict-body --concurrency 16 --delay-ms 5
```
