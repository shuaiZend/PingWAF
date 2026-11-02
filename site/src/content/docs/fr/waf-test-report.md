---
title: Rapport de tests WAF
description: Efficacité mesurée du moteur WAF de PingWAF sur le corpus public chaitin/blazehttp — interception, faux positifs, latence, régression et déterminisme.
minutes: 10
---

## Résumé du verdict

Version 0.32.4, testée le 2026-11-02 sur le jeu de données public [chaitin/blazehttp](https://github.com/chaitin/blazehttp) : 33 877 messages HTTP bruts (658 malveillants, 33 219 bénins). Tous les chiffres ci-dessous utilisent le calibre strict — seule compte comme interception une page de blocage 403 ou une page de défi 503.

| Tier | Interception | blocked | challenged | Bénins affectés |
| --- | --- | --- | --- | --- |
| Normal | 60.2% (396/658) | 396 | 0 | 0.01% (4/33 219) |
| Normal + body | 71.9% (473/658) | 473 | 0 | 0.04% (12) |
| Strict | 76.0% (500/658) | 490 | 10 | 0.01% (4) |
| Strict + body | 93.8% (617/658) | 607 | 10 | 0.06% (19) |

Les taux d'interception sont inchangés par rapport à la passe précédente ; ce sont les nombres d'échantillons bénins affectés qui évoluent avec les correctifs de cette passe. Normal est le tier par défaut et n'inspecte pas le corps des requêtes. Le tier maximal (Strict + inspection du corps) bloque 93.8 % du corpus malveillant public avec un taux de faux positifs de 0.06 %.

## Méthode de test

- **Plan de données statique.** Le moteur démarre depuis une configuration TOML (`pingwaf -c`) avec une origine simulée ; aucun comportement d'agent n'intervient.
- **Replay octet par octet via socket brute.** Les 33 877 échantillons sont rejoués octet par octet sur un socket HTTP/1.1 brut, en préservant l'ordre des en-têtes, les astuces d'encodage et les entrées malformées.
- **Calibre strict.** `blocked` est une page de blocage 403, `challenged` est une page de défi 503 et `intercepted` est leur somme (comparable avec les chiffres historiques). Seuls ces verdicts du WAF comptent ; les rejets au niveau du protocole et les erreurs ne comptent jamais comme interceptions.
- **Calibre effectif.** 20 échantillons malveillants ne peuvent être tranchés par aucun inspecteur côté proxy (POST/PUT sans corps, rejets de protocole, un fragment d'URL). Une fois exclus, le chiffre effectif du tier recommandé est **616/638 = 96.6 %**.

## Interception par catégorie

| Category | n | Normal | Strict+body |
| --- | --- | --- | --- |
| sqli | 20 | 85.0% (17) | 85.0% (17) |
| xss | 30 | 46.7% (14) | 96.7% (29) |
| lfi | 25 | 80.0% (20) | 96.0% (24) |
| rce | 12 | 25.0% (3) | 83.3% (10) |
| ssrf | 13 | 7.7% (1) | 100% (13) |
| other | 538 | 61.9% (333) | 93.9% (505) |

L'écart entre Normal et Strict+body provient des familles dont le payload n'existe que dans le corps de la requête (ssrf 7.7 % → 100 %) et des variantes encodées ou obfusquées (`other` 61.9 % → 93.9 %). L'injection SQL est déjà presque entièrement couverte au tier par défaut.

Sur la suite des payloads compressés (`compressed`), Strict+body intercepte les 8 échantillons malveillants et laisse passer les 2 bénins (2/2) ; Normal+body intercepte 7/8 et manque le payload `$(curl … | sh)` compressé en gzip.

## Régression et déterminisme

- Suite `triage` : 20 échantillons malveillants actuellement manqués épinglés puis rejoués — verdicts identiques à la passe complète, échantillon par échantillon : 19 passent, 1 répond par HTTP 405.
- Suite `fp-pin` : 27 échantillons bénins actuellement bloqués épinglés puis rejoués — 17 toujours bloqués, 2 en défi et 8 qui passent désormais (les faux positifs supprimés dans cette passe).
- `spot-advanced` (chemin advanced_mode) contre Strict+body sur les 33 877 échantillons : **0 différence**, bit à bit.
- Tests unitaires : **250** dans `pingwaf-waf`, tous au vert au moment de la passe.

## Performance et limites

Avec un build release et en loopback, la latence du tier statique reste à **p95 1–2 ms**. Le débit n'est volontairement pas revendiqué : le harnais de replay est un relecteur de fidélité, pas un générateur de charge. Le test couvre uniquement le plan de données TOML statique — le comportement de l'agent (envoi de logs, cookies de passage, escalade vers le blocage) est hors périmètre. Aucun trafic TLS, HTTP/2 ou HTTP/3 n'est exercé.

## Comparaison avec d'autres WAF

Le même corpus public a aussi servi à évaluer d'autres produits. Les chiffres tiers du tableau ci-dessous sont cités tels quels depuis le [README de blazehttp](https://github.com/chaitin/blazehttp) (calibre de 33 669 échantillons) ; les chiffres de PingWAF proviennent de la passe décrite plus haut.

| Produit / mode | Échantillons | Malveillants | Détection | Bénins | Faux positifs | Précision | Temps |
| --- | --- | --- | --- | --- | --- | --- | --- |
| PingWAF Strict+body | 33 877 | 658 | 93.8% (617) | 33 219 | 0.06% (19) | 99.82% | p95 2 ms (loopback) |
| PingWAF Normal | 33 877 | 658 | 60.2% (396) | 33 219 | 0.01% (4) | 99.21% | p95 2 ms (loopback) |
| SafeLine gratuit, strict | 33 669 | 575 | 76.17% (438) | 33 094 | 0.22% (73) | 99.38% | 64.34 ms (public) |
| SafeLine gratuit, équilibré | 33 669 | 575 | 71.65% (412) | 33 094 | 0.07% (23) | 99.45% | 70.05 ms (public) |
| ModSecurity PL4 | 33 669 | 575 | 94.61% (544) | 33 094 | 52.46% (17 362) | 48.34% | 28.89 ms (public) |
| ModSecurity PL1 | 33 669 | 575 | 69.74% (401) | 33 094 | 17.58% (5 819) | 82.20% | 31.15 ms (public) |
| Cloudflare gratuit | 33 669 | 570 | 10.70% (61) | 32 780 | 0.07% (23) | 98.40% | 288.96 ms (public) |

Comment lire ce tableau :

1. Les instantanés d'échantillons diffèrent (officiel : 33 669 échantillons avec 570–575 malveillants ; PingWAF : 33 877 avec 658), les taux de détection ne partagent donc pas le même dénominateur.
2. Le temps se mesure différemment : les chiffres en réseau public incluent le RTT, ceux de PingWAF sont la latence de verdict en loopback — aucun des deux n'est une comparaison de débit.
3. Les chiffres tiers sont cités tels quels et n'ont pas été retestés dans cette passe ; le tableau décrit uniquement la position relative de chaque moteur sur le même corpus public.

## Reproduction et rapport complet

Le rapport complet, avec l'attribution des faux positifs règle par règle et l'analyse des artefacts, vit dans le dépôt :

- [Rapport complet de tests (EN)](https://github.com/shuaiZend/PingWAF/blob/main/docs/waf-test-report.md)
- [Plan de tests et définition des calibres](https://github.com/shuaiZend/PingWAF/blob/main/docs/waf-test-plan.md)

La méthodologie complète et la définition des calibres figurent dans le
[plan de tests](https://github.com/shuaiZend/PingWAF/blob/main/docs/waf-test-plan.md).
