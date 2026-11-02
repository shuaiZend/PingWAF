# waf-bench summary

## Overall (per group)

`blocked` = HTTP 403 block page; `challenged` = HTTP 503 challenge page; `intercepted` = blocked + challenged.

| group | total | black | white | black blocked | black challenged | intercept rate | white blocked | white challenged | FP rate (blocked) | FP rate (intercepted) | p50 ms | p95 ms | error % | status_405 | status_500 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| fp-pin | 27 | 0 | 27 | 0 | 0 | 0.0% | 17 | 2 | 62.96% | 70.37% | 1 | 4 | 0.00% | 0 | 0 |

## fp-pin: black samples by category

| category | n | blocked | challenged | intercepted | +reject/error | intercept rate |
|---|---|---|---|---|---|---|

## fp-pin: white false positives by category

| category | n | blocked | challenged | FP rate (intercepted) | examples |
|---|---|---|---|---|---|
| other | 12 | 3 | 2 | 41.67% | `07/cd/61fa0932f7c58ecae04b8506c1d9.white`, `4c/7c/5c3413c84dad8655aebd603d1b8e.white`, `5e/cc/8f95195e047fe555fb0b8775eb7b.white`, `b6/5f/3539ffaf5a5316408d5dcf38e0fc.white`, `e9/75/6d588c1fcc5baa1baee6ea139e49.white` |
| rce | 5 | 5 | 0 | 100.00% | `2b/19/616c4c2a8288ed28d4d4566f71f8.white`, `59/30/ae3e988a6ed4a9ae2c571eb001ef.white`, `73/29/62f8a14a1887f13f097d9e8ac310.white`, `a4/15/ce94a553f132de70f988405dd932.white`, `bb/79/83610bba10cf1fed5c571093c838.white` |
| xss | 3 | 3 | 0 | 100.00% | `40/ea/df246aa220caef04ae54a1eac6ae.white`, `53/8d/8aaede20848d0877bad6b4be6acc.white`, `80/38/8c27ccc89c23fb4ea04ed7ae4be2.white` |
| lfi | 3 | 3 | 0 | 100.00% | `46/8b/c07bd371d381b197928bca36cb94.white`, `73/e6/24ff969cb99ec070e4d65861e311.white`, `7e/ac/a54c85a827388378627dc1e0daa2.white` |
| crlf | 1 | 1 | 0 | 100.00% | `52/5c/5a044ff90b8548e753b3ffc47ce2.white` |
| ssti | 1 | 1 | 0 | 100.00% | `61/81/472215d2fca9ef868561906345b2.white` |
| sqli | 1 | 1 | 0 | 100.00% | `92/49/913922b6c313eb1bc7c594324578da3f.white` |

## fp-pin: missed black samples (passed) by category

| category | missed | example files |
|---|---|---|
