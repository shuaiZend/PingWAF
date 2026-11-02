# waf-bench summary

## Overall (per group)

`blocked` = HTTP 403 block page; `challenged` = HTTP 503 challenge page; `intercepted` = blocked + challenged.

| group | total | black | white | black blocked | black challenged | intercept rate | white blocked | white challenged | FP rate (blocked) | FP rate (intercepted) | p50 ms | p95 ms | error % | status_405 | status_500 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| strict-body | 33877 | 658 | 33219 | 607 | 10 | 93.8% | 17 | 2 | 0.05% | 0.06% | 1 | 2 | 0.00% | 1 | 15 |

## strict-body: black samples by category

| category | n | blocked | challenged | intercepted | +reject/error | intercept rate |
|---|---|---|---|---|---|---|
| other | 538 | 499 | 6 | 505 | 1 | 93.9% |
| xss | 30 | 29 | 0 | 29 | 0 | 96.7% |
| lfi | 25 | 23 | 1 | 24 | 0 | 96.0% |
| sqli | 20 | 17 | 0 | 17 | 1 | 85.0% |
| ssrf | 13 | 13 | 0 | 13 | 0 | 100.0% |
| rce | 12 | 8 | 2 | 10 | 0 | 83.3% |
| deser | 6 | 6 | 0 | 6 | 0 | 100.0% |
| crlf | 5 | 5 | 0 | 5 | 0 | 100.0% |
| ssti | 5 | 5 | 0 | 5 | 0 | 100.0% |
| xxe | 2 | 0 | 1 | 1 | 0 | 50.0% |
| log4shell | 1 | 1 | 0 | 1 | 0 | 100.0% |
| scanner | 1 | 1 | 0 | 1 | 0 | 100.0% |

## strict-body: white false positives by category

| category | n | blocked | challenged | FP rate (intercepted) | examples |
|---|---|---|---|---|---|
| other | 32432 | 3 | 2 | 0.02% | `07/cd/61fa0932f7c58ecae04b8506c1d9.white`, `4c/7c/5c3413c84dad8655aebd603d1b8e.white`, `5e/cc/8f95195e047fe555fb0b8775eb7b.white`, `b6/5f/3539ffaf5a5316408d5dcf38e0fc.white`, `e9/75/6d588c1fcc5baa1baee6ea139e49.white` |
| rce | 460 | 5 | 0 | 1.09% | `2b/19/616c4c2a8288ed28d4d4566f71f8.white`, `59/30/ae3e988a6ed4a9ae2c571eb001ef.white`, `73/29/62f8a14a1887f13f097d9e8ac310.white`, `a4/15/ce94a553f132de70f988405dd932.white`, `bb/79/83610bba10cf1fed5c571093c838.white` |
| xss | 50 | 3 | 0 | 6.00% | `40/ea/df246aa220caef04ae54a1eac6ae.white`, `53/8d/8aaede20848d0877bad6b4be6acc.white`, `80/38/8c27ccc89c23fb4ea04ed7ae4be2.white` |
| lfi | 4 | 3 | 0 | 75.00% | `46/8b/c07bd371d381b197928bca36cb94.white`, `73/e6/24ff969cb99ec070e4d65861e311.white`, `7e/ac/a54c85a827388378627dc1e0daa2.white` |
| sqli | 67 | 1 | 0 | 1.49% | `92/49/913922b6c313eb1bc7c594324578da3f.white` |
| crlf | 73 | 1 | 0 | 1.37% | `52/5c/5a044ff90b8548e753b3ffc47ce2.white` |
| ssti | 4 | 1 | 0 | 25.00% | `61/81/472215d2fca9ef868561906345b2.white` |

## strict-body: missed black samples (passed) by category

| category | missed | example files |
|---|---|---|
| other | 33 | `3e/3e/d7b61c3dfcd450e63e1cbeca4058.black`, `4b/0a/096825fe5d589a0fe75c44aca53e.black`, `56/e4/0a547696d93b1a50b4533c4bcfa2.black`, `6b/2a/83249e94cbaa8d93929c46c426bf.black`, `73/81/da87ccf03256c0a4f316afe35e04.black` |
| xss | 1 | `9d/fb/d8758da86a7e57209ade8d184b3b.black` |
| lfi | 1 | `c0/89/6cbe70f48c5fd7492607415e1918.black` |
| sqli | 3 | `ce/8e/1107e19a71beb168c4133f415dea.black` |
| rce | 2 | `4f/7f/c352b5295ad70d7c460cb2d1c320.black`, `de/16/8f5208d44b701dd9e7e7b5245d08.black` |
| xxe | 1 | `c2/14/44c7ca191769289924c18167598b.black` |
