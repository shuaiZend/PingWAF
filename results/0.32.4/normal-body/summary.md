# waf-bench summary

## Overall (per group)

`blocked` = HTTP 403 block page; `challenged` = HTTP 503 challenge page; `intercepted` = blocked + challenged.

| group | total | black | white | black blocked | black challenged | intercept rate | white blocked | white challenged | FP rate (blocked) | FP rate (intercepted) | p50 ms | p95 ms | error % | status_405 | status_500 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| normal-body | 33877 | 658 | 33219 | 473 | 0 | 71.9% | 10 | 2 | 0.03% | 0.04% | 0 | 2 | 0.00% | 1 | 15 |

## normal-body: black samples by category

| category | n | blocked | challenged | intercepted | +reject/error | intercept rate |
|---|---|---|---|---|---|---|
| other | 538 | 392 | 0 | 392 | 1 | 72.9% |
| xss | 30 | 14 | 0 | 14 | 0 | 46.7% |
| lfi | 25 | 22 | 0 | 22 | 0 | 88.0% |
| sqli | 20 | 17 | 0 | 17 | 1 | 85.0% |
| ssrf | 13 | 8 | 0 | 8 | 0 | 61.5% |
| rce | 12 | 6 | 0 | 6 | 0 | 50.0% |
| deser | 6 | 6 | 0 | 6 | 0 | 100.0% |
| crlf | 5 | 3 | 0 | 3 | 0 | 60.0% |
| ssti | 5 | 3 | 0 | 3 | 0 | 60.0% |
| xxe | 2 | 1 | 0 | 1 | 0 | 50.0% |
| log4shell | 1 | 1 | 0 | 1 | 0 | 100.0% |
| scanner | 1 | 0 | 0 | 0 | 0 | 0.0% |

## normal-body: white false positives by category

| category | n | blocked | challenged | FP rate (intercepted) | examples |
|---|---|---|---|---|---|
| other | 32432 | 3 | 2 | 0.02% | `07/cd/61fa0932f7c58ecae04b8506c1d9.white`, `4c/7c/5c3413c84dad8655aebd603d1b8e.white`, `5e/cc/8f95195e047fe555fb0b8775eb7b.white`, `b6/5f/3539ffaf5a5316408d5dcf38e0fc.white`, `e9/75/6d588c1fcc5baa1baee6ea139e49.white` |
| xss | 50 | 3 | 0 | 6.00% | `40/ea/df246aa220caef04ae54a1eac6ae.white`, `53/8d/8aaede20848d0877bad6b4be6acc.white`, `80/38/8c27ccc89c23fb4ea04ed7ae4be2.white` |
| rce | 460 | 2 | 0 | 0.43% | `2b/19/616c4c2a8288ed28d4d4566f71f8.white`, `73/29/62f8a14a1887f13f097d9e8ac310.white` |
| sqli | 67 | 1 | 0 | 1.49% | `92/49/913922b6c313eb1bc7c594324578da3f.white` |
| crlf | 73 | 1 | 0 | 1.37% | `52/5c/5a044ff90b8548e753b3ffc47ce2.white` |

## normal-body: missed black samples (passed) by category

| category | missed | example files |
|---|---|---|
| other | 146 | `01/cd/82de0969852cbfec551a407988a5.black`, `05/4a/18ddcb45abc0d92107ff3897da0b.black`, `08/1f/762a42518f7a5aab3bd981a9bed9.black`, `0d/4e/d43b2447003794077be8f6409476.black`, `0e/0b/41203714a84d6a9e521d6fb4e0be.black` |
| xss | 16 | `00/e1/1e708ad1f095e8e21f57f4bb8cfc.black`, `05/ff/86504f3eb1098c3360626b095149.black`, `10/bc/219d70ebdd8b498dc0f2bedbd36e.black`, `32/cc/67d2c672ab4a88d5a845bdcf6081.black`, `35/ac/8a0d385b0b52ef229a1cf21b3e11.black` |
| lfi | 3 | `29/2b/62b4f2b39173e65cc2cf8c76ad3c.black`, `35/df/d5d308be124938cf82bb2ab02c32.black`, `c0/89/6cbe70f48c5fd7492607415e1918.black` |
| sqli | 3 | `ce/8e/1107e19a71beb168c4133f415dea.black` |
| ssrf | 5 | `38/57/251126732b39d45ef25e5778240c.black`, `4f/6e/13e7a46998deda814e5cdcec5473.black`, `7d/20/0082bcd4fd13a45491c4f1af6a15.black`, `9a/59/e5a90a2ce7e2b45bc8b2986d6f19.black`, `e4/63/cde9e9f93c5ccd360fb98dfd5d4e.black` |
| rce | 6 | `29/fc/c5a02830dd154507018058dad3af.black`, `4f/7f/c352b5295ad70d7c460cb2d1c320.black`, `5b/36/bf087c5bf71d51228b4ab5916127.black`, `c7/8a/dcf02bbc575d68ce9ddcd72611b2.black`, `d5/cd/8b5a4e82f781e18c753abaceb807.black` |
| crlf | 2 | `52/5a/c7c887b271e38260d483f503e2a6.black`, `a2/74/775a346c2b3091f48859e1785524.black` |
| ssti | 2 | `5b/0d/13f7d5f4c37d750a1e23b19a9cb8.black`, `d6/36/4f7fd442d0ded1ed975c5131a5a7.black` |
| xxe | 1 | `c2/14/44c7ca191769289924c18167598b.black` |
| scanner | 1 | `5b/06/b33273adabbcb06926720e04cc1f.black` |
