/**
 * Cross-vendor benchmark rows for the WAF test report page.
 *
 * PingWAF figures come from `results/0.32.4/metrics.json` (33 877 samples,
 * this repository's own replay). The other rows are quoted from the blazehttp
 * project README (33 669 samples), which is why the page shows a source note
 * next to the charts. Labels are product names and stay in English across
 * locales; only the prose around them is translated.
 */
export interface BenchRow {
  label: string;
  /** Percent, 0-100. */
  value: number;
  /** PingWAF rows carry the site accent colour. */
  accent?: boolean;
}

export const INTERCEPTION_ROWS: BenchRow[] = [
  { label: "PingWAF Strict+body", value: 93.8, accent: true },
  { label: "PingWAF Normal", value: 60.2, accent: true },
  { label: "SafeLine strict", value: 76.17 },
  { label: "SafeLine balanced", value: 71.65 },
  { label: "ModSecurity PL4", value: 94.61 },
  { label: "ModSecurity PL1", value: 69.74 },
  { label: "Cloudflare free", value: 10.7 },
];

export const FALSE_POSITIVE_ROWS: BenchRow[] = [
  { label: "PingWAF Strict+body", value: 0.06, accent: true },
  { label: "PingWAF Normal", value: 0.01, accent: true },
  { label: "SafeLine strict", value: 0.22 },
  { label: "SafeLine balanced", value: 0.07 },
  { label: "ModSecurity PL4", value: 52.46 },
  { label: "ModSecurity PL1", value: 17.58 },
  { label: "Cloudflare free", value: 0.07 },
];

/** Scale ceilings, chosen so the longest bar sits just below the rail end. */
export const INTERCEPTION_MAX = 100;
export const FALSE_POSITIVE_MAX = 55;
