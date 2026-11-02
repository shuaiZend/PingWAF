# waf-bench summary

## Overall (per group)

`blocked` = HTTP 403 block page; `challenged` = HTTP 503 challenge page; `intercepted` = blocked + challenged.

| group | total | black | white | black blocked | black challenged | intercept rate | white blocked | white challenged | FP rate (blocked) | FP rate (intercepted) | p50 ms | p95 ms | error % | status_405 | status_500 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| normal | 33877 | 658 | 33219 | 396 | 0 | 60.2% | 2 | 2 | 0.01% | 0.01% | 0 | 2 | 0.00% | 1 | 15 |

## normal: black samples by category

| category | n | blocked | challenged | intercepted | +reject/error | intercept rate |
|---|---|---|---|---|---|---|
| other | 538 | 333 | 0 | 333 | 1 | 61.9% |
| xss | 30 | 14 | 0 | 14 | 0 | 46.7% |
| lfi | 25 | 20 | 0 | 20 | 0 | 80.0% |
| sqli | 20 | 17 | 0 | 17 | 1 | 85.0% |
| ssrf | 13 | 1 | 0 | 1 | 0 | 7.7% |
| rce | 12 | 3 | 0 | 3 | 0 | 25.0% |
| deser | 6 | 1 | 0 | 1 | 0 | 16.7% |
| crlf | 5 | 3 | 0 | 3 | 0 | 60.0% |
| ssti | 5 | 3 | 0 | 3 | 0 | 60.0% |
| xxe | 2 | 0 | 0 | 0 | 0 | 0.0% |
| log4shell | 1 | 1 | 0 | 1 | 0 | 100.0% |
| scanner | 1 | 0 | 0 | 0 | 0 | 0.0% |

## normal: white false positives by category

| category | n | blocked | challenged | FP rate (intercepted) | examples |
|---|---|---|---|---|---|
| other | 32432 | 0 | 2 | 0.01% | `4c/7c/5c3413c84dad8655aebd603d1b8e.white`, `5e/cc/8f95195e047fe555fb0b8775eb7b.white` |
| sqli | 67 | 1 | 0 | 1.49% | `92/49/913922b6c313eb1bc7c594324578da3f.white` |
| rce | 460 | 1 | 0 | 0.22% | `2b/19/616c4c2a8288ed28d4d4566f71f8.white` |

## normal: missed black samples (passed) by category

| category | missed | example files |
|---|---|---|
| other | 205 | `01/cd/82de0969852cbfec551a407988a5.black`, `05/4a/18ddcb45abc0d92107ff3897da0b.black`, `05/4f/c6068a4cd81914b8d0932213e086.black`, `07/c9/4af082d5449db3db2611bdf4ec28.black`, `08/1f/762a42518f7a5aab3bd981a9bed9.black` |
| xss | 16 | `00/e1/1e708ad1f095e8e21f57f4bb8cfc.black`, `05/ff/86504f3eb1098c3360626b095149.black`, `10/bc/219d70ebdd8b498dc0f2bedbd36e.black`, `32/cc/67d2c672ab4a88d5a845bdcf6081.black`, `35/ac/8a0d385b0b52ef229a1cf21b3e11.black` |
| lfi | 5 | `29/2b/62b4f2b39173e65cc2cf8c76ad3c.black`, `35/df/d5d308be124938cf82bb2ab02c32.black`, `c0/89/6cbe70f48c5fd7492607415e1918.black`, `c3/c5/dae4b35105c6fe571cccc511cfe1.black`, `ec/32/ffe6a8aac5230afc7628248c70d6.black` |
| sqli | 3 | `ce/8e/1107e19a71beb168c4133f415dea.black` |
| ssrf | 12 | `38/57/251126732b39d45ef25e5778240c.black`, `4f/6e/13e7a46998deda814e5cdcec5473.black`, `59/51/176081cb991309741a15edec2639.black`, `5a/1b/d92a6e48a01d9dbf6704d7ae9f3f.black`, `64/b5/712a75fb36bec21460ca6496afe9.black` |
| rce | 9 | `03/57/9d832d81d125f054d3e0593d944f.black`, `29/fc/c5a02830dd154507018058dad3af.black`, `2f/f0/1c77d9cb502c02a442ba36b0ad11.black`, `4f/7f/c352b5295ad70d7c460cb2d1c320.black`, `5b/36/bf087c5bf71d51228b4ab5916127.black` |
| deser | 5 | `06/da/18edf5c23081f8e2e679ce498858.black`, `17/9a/3ca003cd6941525e4293e84669f3.black`, `2d/c7/7c87e5745309e6615dc85506105e.black`, `45/c6/efe7e29662f1b708a8782bdb478c.black`, `cb/38/47cf1040b674f20d9a542ac321ec.black` |
| crlf | 2 | `52/5a/c7c887b271e38260d483f503e2a6.black`, `a2/74/775a346c2b3091f48859e1785524.black` |
| ssti | 2 | `5b/0d/13f7d5f4c37d750a1e23b19a9cb8.black`, `d6/36/4f7fd442d0ded1ed975c5131a5a7.black` |
| xxe | 2 | `89/67/48f3d90b343453994e53e942b2d9.black`, `c2/14/44c7ca191769289924c18167598b.black` |
| scanner | 1 | `5b/06/b33273adabbcb06926720e04cc1f.black` |
