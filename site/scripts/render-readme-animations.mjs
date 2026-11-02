#!/usr/bin/env node
/**
 * Renders the two animated WebP assets used by the repository READMEs.
 *
 *   node site/scripts/render-readme-animations.mjs                     # both scenes
 *   node site/scripts/render-readme-animations.mjs --only=architecture  # one scene
 *
 * Each scene is an inline HTML document with pure CSS keyframes. Every frame is
 * captured with the animations paused and `currentTime` set explicitly, so the
 * 96 frames map exactly onto the 8 s timeline — recording would drop or repeat
 * frames under load. Frames are written to a system temp directory and encoded
 * with ffmpeg's libwebp_anim; nothing but the two .webp files is committed.
 *
 * Requirements: Playwright (reused from e2e/ or site/node_modules) and a system
 * ffmpeg with libwebp_anim. No npm dependencies are added by this script.
 */
import { execFile } from "node:child_process";
import { mkdtemp, rm, mkdir, stat } from "node:fs/promises";
import { createRequire } from "node:module";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

const run = promisify(execFile);

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "../..");
const assetDir = path.join(repoRoot, "asset");

const FPS = 12;
const SECONDS = 8;
const FRAMES = FPS * SECONDS;
const QUALITY = 72;

/** Shared palette — dark console surface, readable under both GitHub themes. */
const THEME = {
  bg: "#101010",
  grid: "#171717",
  panel: "#191919",
  raised: "#202020",
  line: "#2e2e2e",
  text: "#e8e8e8",
  muted: "#8d8d8d",
  brand: "#f6821f",
  green: "#30a46c",
  red: "#ff6340",
  cyan: "#52a9ff",
};

const SCENES = {
  architecture: { width: 1000, height: 560 },
  effectiveness: { width: 1000, height: 400 },
};

function loadPlaywright() {
  const candidates = [
    path.join(repoRoot, "e2e", "package.json"),
    path.join(repoRoot, "web", "package.json"),
    path.join(here, "..", "package.json"),
  ];
  for (const candidate of candidates) {
    try {
      const require = createRequire(candidate);
      return require("@playwright/test");
    } catch {
      // try the next resolution root
    }
  }
  throw new Error("Playwright not found — install @playwright/test or reuse e2e/node_modules.");
}

const baseCss = (width, height) => `
  * { box-sizing: border-box; }
  html, body { margin: 0; padding: 0; }
  body {
    width: ${width}px;
    height: ${height}px;
    overflow: hidden;
    background: ${THEME.bg};
    background-image:
      linear-gradient(to right, ${THEME.grid} 1px, transparent 1px),
      linear-gradient(to bottom, ${THEME.grid} 1px, transparent 1px);
    background-size: 40px 40px;
    font-family: ui-monospace, "SF Mono", SFMono-Regular, Menlo, Consolas, monospace;
    color: ${THEME.text};
    -webkit-font-smoothing: antialiased;
  }
  .stage { position: relative; width: ${width}px; height: ${height}px; }
  .wires { position: absolute; inset: 0; width: 100%; height: 100%; }
  .wire { fill: none; stroke: ${THEME.line}; stroke-width: 1.4; }
  .node {
    position: absolute;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 3px;
    padding: 10px 12px;
    border: 1px solid ${THEME.line};
    border-radius: 10px;
    background: ${THEME.panel};
  }
  .node--group {
    justify-content: flex-start;
    padding: 12px 0 0;
    background: transparent;
    border-color: ${THEME.line};
  }
  .node__label {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.1em;
    color: ${THEME.muted};
    text-transform: uppercase;
  }
  .node__name { font-size: 13px; color: ${THEME.text}; }
  .node__meta { font-size: 10px; color: ${THEME.muted}; }
  .group__title {
    padding: 0 12px;
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.1em;
    color: ${THEME.muted};
    text-transform: uppercase;
  }
  .dot {
    position: absolute;
    top: 0;
    left: 0;
    width: 7px;
    height: 7px;
    margin: -3.5px 0 0 -3.5px;
    border-radius: 50%;
    offset-rotate: 0deg;
    opacity: 0;
  }
  .dot--req { background: ${THEME.brand}; box-shadow: 0 0 10px ${THEME.brand}; }
  .dot--safe { background: ${THEME.green}; box-shadow: 0 0 10px ${THEME.green}; }
  .dot--grpc { background: ${THEME.cyan}; box-shadow: 0 0 10px ${THEME.cyan}; }
  .dot--write { background: ${THEME.muted}; box-shadow: 0 0 8px ${THEME.muted}; }
`;

/* ---------------------------------------------------------------- architecture */

function architectureHtml() {
  const { width, height } = SCENES.architecture;
  return `<!doctype html><html><head><meta charset="utf-8"><style>
${baseCss(width, height)}
  /* Client → agents */
  .dot--req-a { offset-path: path("M180 280 C 205 280, 218 195, 240 195"); animation: travel-a 8s linear infinite; }
  .dot--req-b { offset-path: path("M180 280 C 205 280, 218 315, 240 315"); animation: travel-a 8s linear infinite; animation-delay: -0.6s; }
  /* Agents → origin, safe traffic */
  .dot--safe-a { offset-path: path("M370 372 C 430 528, 660 548, 820 500"); animation: travel-safe 8s linear infinite; animation-delay: -2.4s; }
  .dot--safe-b { offset-path: path("M370 372 C 430 528, 660 548, 820 500"); animation: travel-safe 8s linear infinite; animation-delay: -4.6s; }
  /* Agents ↔ control plane, gRPC streams both ways */
  .dot--up-a { offset-path: path("M500 190 C 540 190, 556 255, 580 255"); animation: travel-grpc 8s linear infinite; animation-delay: -0.4s; }
  .dot--up-b { offset-path: path("M500 302 C 540 302, 556 255, 580 255"); animation: travel-grpc 8s linear infinite; animation-delay: -2.2s; }
  .dot--down { offset-path: path("M580 236 C 556 236, 540 168, 500 168"); animation: travel-grpc 8s linear infinite; animation-delay: -4.4s; }
  /* Control plane → storage */
  .dot--write-pg { offset-path: path("M700 264 L 660 330"); animation: travel-write 2s linear infinite; }
  .dot--write-es { offset-path: path("M820 264 L 845 330"); animation: travel-write 2s linear infinite; animation-delay: -1s; }

  @keyframes travel-a {
    0% { offset-distance: 0%; opacity: 0; }
    2% { opacity: 1; }
    18% { offset-distance: 100%; opacity: 0; }
    100% { offset-distance: 100%; opacity: 0; }
  }
  @keyframes travel-safe {
    0%, 22% { offset-distance: 0%; opacity: 0; }
    26% { opacity: 1; }
    46% { offset-distance: 100%; opacity: 0; }
    100% { offset-distance: 100%; opacity: 0; }
  }
  @keyframes travel-grpc {
    0% { offset-distance: 0%; opacity: 0; }
    6% { opacity: 1; }
    92% { offset-distance: 100%; opacity: 0; }
    100% { offset-distance: 100%; opacity: 0; }
  }
  @keyframes travel-write {
    0% { offset-distance: 0%; opacity: 0; }
    20% { opacity: 0.9; }
    80% { offset-distance: 100%; opacity: 0; }
    100% { offset-distance: 100%; opacity: 0; }
  }

  /* Scan sweep inside an agent, plus the blocked sample */
  .scan { position: absolute; left: 0; right: 0; height: 1px; background: linear-gradient(90deg, transparent, ${THEME.brand}, transparent); opacity: 0; }
  .scan--a { top: 54px; animation: scan 8s ease-in-out infinite; }
  @keyframes scan {
    0%, 16% { opacity: 0; transform: translateY(0); }
    20% { opacity: 0.9; }
    30% { opacity: 0; transform: translateY(78px); }
    100% { opacity: 0; transform: translateY(78px); }
  }
  .block {
    position: absolute;
    left: 186px;
    top: 58px;
    padding: 2px 6px;
    border-radius: 4px;
    border: 1px solid ${THEME.red};
    background: rgba(255, 99, 64, 0.14);
    color: ${THEME.red};
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.06em;
    opacity: 0;
    animation: block 8s steps(1, end) infinite;
  }
  @keyframes block {
    0%, 20% { opacity: 0; }
    22%, 34% { opacity: 1; }
    38%, 100% { opacity: 0; }
  }
  .pulse { animation: pulse 2s ease-in-out infinite; }
  @keyframes pulse {
    0%, 100% { border-color: ${THEME.line}; }
    50% { border-color: ${THEME.cyan}; }
  }
  .eyebrow { position: absolute; left: 30px; top: 26px; font-size: 10px; letter-spacing: 0.14em; color: ${THEME.muted}; text-transform: uppercase; }
  .legend { position: absolute; right: 30px; top: 26px; display: flex; gap: 14px; font-size: 10px; color: ${THEME.muted}; }
  .legend i { display: inline-block; width: 7px; height: 7px; border-radius: 50%; margin-right: 5px; vertical-align: 1px; }
</style></head><body><div class="stage">
  <p class="eyebrow">PingWAF · control plane + edge agents</p>
  <div class="legend">
    <span><i style="background:${THEME.brand}"></i>request</span>
    <span><i style="background:${THEME.green}"></i>safe traffic</span>
    <span><i style="background:${THEME.cyan}"></i>gRPC stream</span>
  </div>

  <svg class="wires" viewBox="0 0 ${width} ${height}">
    <path class="wire" d="M180 280 C 205 280, 218 195, 240 195" />
    <path class="wire" d="M180 280 C 205 280, 218 315, 240 315" />
    <path class="wire" d="M500 190 C 540 190, 556 255, 580 255" />
    <path class="wire" d="M500 302 C 540 302, 556 255, 580 255" />
    <path class="wire" d="M370 372 C 430 528, 660 548, 820 500" stroke="${THEME.green}" stroke-opacity="0.4" />
    <path class="wire" d="M700 264 L 660 330" />
    <path class="wire" d="M820 264 L 845 330" />
  </svg>

  <div class="node" style="left:30px;top:240px;width:150px;height:80px">
    <span class="node__label">Client</span>
    <span class="node__name">Browser / API</span>
    <span class="node__meta">HTTP · HTTPS</span>
  </div>

  <div class="node node--group" style="left:220px;top:100px;width:300px;height:300px">
    <span class="group__title">Data plane · agents</span>
    <div class="node" style="left:20px;top:46px;width:260px;height:88px">
      <span class="node__label">Edge agent A</span>
      <span class="node__name">waf engine · cache</span>
      <span class="node__meta">:80 / :443</span>
    </div>
    <div class="node" style="left:20px;top:158px;width:260px;height:88px">
      <span class="node__label">Edge agent B</span>
      <span class="node__name">waf engine · cache</span>
      <span class="node__meta">:80 / :443</span>
    </div>
    <div class="scan scan--a"></div>
    <div class="block">BLOCK</div>
  </div>

  <div class="node node--group pulse" style="left:560px;top:100px;width:400px;height:300px">
    <span class="group__title">Control plane · pingwaf server</span>
    <div class="node" style="left:20px;top:46px;width:360px;height:58px">
      <span class="node__label">Embedded console + REST</span>
      <span class="node__meta">:9080 · rust-embed</span>
    </div>
    <div class="node" style="left:20px;top:126px;width:360px;height:58px">
      <span class="node__label">gRPC ControlPlane</span>
      <span class="node__meta">:9090 · 7 RPC</span>
    </div>
    <div class="node" style="left:20px;top:206px;width:170px;height:58px">
      <span class="node__label">PostgreSQL</span>
      <span class="node__meta">state</span>
    </div>
    <div class="node" style="left:200px;top:206px;width:180px;height:58px">
      <span class="node__label">Elasticsearch</span>
      <span class="node__meta">optional</span>
    </div>
  </div>

  <div class="node" style="left:820px;top:462px;width:160px;height:76px">
    <span class="node__label">Origin</span>
    <span class="node__name">upstream pool</span>
    <span class="node__meta">safe traffic only</span>
  </div>

  <div class="dot dot--req dot--req-a"></div>
  <div class="dot dot--req dot--req-b"></div>
  <div class="dot dot--safe dot--safe-a"></div>
  <div class="dot dot--safe dot--safe-b"></div>
  <div class="dot dot--grpc dot--up-a"></div>
  <div class="dot dot--grpc dot--up-b"></div>
  <div class="dot dot--grpc dot--down"></div>
  <div class="dot dot--write dot--write-pg"></div>
  <div class="dot dot--write dot--write-es"></div>
</div></body></html>`;
}

/* --------------------------------------------------------------- effectiveness */

const BARS = [
  { label: "PingWAF Strict + body", value: 93.8, accent: true, effective: 96.6 },
  { label: "ModSecurity PL4", value: 94.61 },
  { label: "SafeLine strict", value: 76.17 },
  { label: "ModSecurity PL1", value: 69.74 },
  { label: "Cloudflare free", value: 10.7, note: "10.70" },
];

function effectivenessHtml() {
  const { width, height } = SCENES.effectiveness;
  const track = 620;
  const rows = BARS.map((bar, index) => {
    const left = 250;
    const top = 92 + index * 52;
    const w = (bar.value / 100).toFixed(4);
    const delay = (index * 0.28).toFixed(2);
    const extend = bar.effective ? ((bar.effective - bar.value) / 100) * track : 0;
    const valueLeft = (bar.value / 100) * track + extend + 10;
    const text = bar.note ?? String(bar.value);
    return `
    <div class="row" style="top:${top}px">
      <span class="row__label${bar.accent ? " row__label--accent" : ""}">${bar.label}</span>
      <div class="bar" style="left:${left}px;width:${track}px">
        <div class="bar__fill${bar.accent ? " bar__fill--accent" : ""}" style="--w:${w};animation-delay:${delay}s"></div>
        ${
          bar.accent
            ? `<div class="bar__extend" style="left:${(bar.value / 100) * track}px;width:${extend}px;animation-delay:${delay}s"></div>`
            : ""
        }
        <span class="bar__value${bar.accent ? " bar__value--accent" : ""}" style="left:${valueLeft}px;animation-delay:${delay}s">${text}%</span>
        ${
          bar.accent
            ? `<span class="bar__note" style="left:${valueLeft + 48}px;animation-delay:${delay}s">96.6% eff.</span>`
            : ""
        }
      </div>
    </div>`;
  }).join("");

  return `<!doctype html><html><head><meta charset="utf-8"><style>
${baseCss(width, height)}
  .head { position: absolute; left: 30px; top: 26px; }
  .head__title { margin: 0; font-size: 15px; font-weight: 600; color: ${THEME.text}; }
  .head__sub { margin: 6px 0 0; font-size: 10px; color: ${THEME.muted}; }
  .row { position: absolute; left: 30px; right: 30px; height: 40px; }
  .row__label {
    position: absolute; left: 0; top: 10px; width: 210px;
    font-size: 11.5px; color: ${THEME.muted};
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  .row__label--accent { color: ${THEME.text}; font-weight: 600; }
  .bar { position: absolute; top: 10px; height: 18px; }
  .bar__fill {
    position: absolute; inset: 0;
    border-radius: 4px;
    background: #2b2b2b;
    transform-origin: left center;
    transform: scaleX(0);
    animation: grow 8s cubic-bezier(0.22, 0.8, 0.24, 1) infinite;
  }
  .bar__fill--accent { background: linear-gradient(90deg, ${THEME.brand}, #ffa45c); }
  .bar__extend {
    position: absolute; top: 10px; height: 18px;
    border-radius: 4px;
    background: repeating-linear-gradient(45deg, rgba(246, 130, 31, 0.55) 0 4px, rgba(246, 130, 31, 0.18) 4px 8px);
    opacity: 0;
    animation: extend 8s ease-out infinite;
  }
  .bar__value {
    position: absolute; top: 12px;
    white-space: nowrap;
    font-size: 11px; color: ${THEME.muted};
    opacity: 0;
    animation: reveal 8s ease-out infinite;
  }
  .bar__value--accent { color: ${THEME.brand}; font-weight: 700; }
  .bar__note {
    position: absolute; top: 13px;
    white-space: nowrap;
    font-size: 9.5px; color: ${THEME.brand}; opacity: 0;
    animation: reveal 8s ease-out infinite;
  }
  @keyframes grow {
    0% { transform: scaleX(0); }
    24% { transform: scaleX(var(--w)); }
    100% { transform: scaleX(var(--w)); }
  }
  @keyframes extend {
    0%, 26% { opacity: 0; }
    34%, 100% { opacity: 1; }
  }
  @keyframes reveal {
    0%, 14% { opacity: 0; }
    22%, 100% { opacity: 1; }
  }
  .foot {
    position: absolute; left: 30px; bottom: 26px;
    font-size: 10px; color: ${THEME.muted};
  }
  .foot b { color: ${THEME.text}; font-weight: 600; }
</style></head><body><div class="stage">
  <div class="head">
    <h1 class="head__title">Interception · blazehttp corpus</h1>
    <p class="head__sub">same corpus as the table below · 658 malicious samples · strict + body inspection</p>
  </div>
  ${rows}
  <p class="foot"><b>PingWAF</b> · 0.06 % benign affected · 99.82 % accuracy · effective interception <b>96.6 %</b> (616/638)</p>
</div></body></html>`;
}

/* ------------------------------------------------------------------- rendering */

async function renderScene(browser, scene) {
  const { width, height } = SCENES[scene];
  const html = scene === "architecture" ? architectureHtml() : effectivenessHtml();
  const dir = await mkdtemp(path.join(os.tmpdir(), `pingwaf-${scene}-`));

  const page = await browser.newPage({ viewport: { width, height }, deviceScaleFactor: 1 });
  await page.setContent(html, { waitUntil: "load" });
  await page.evaluate(() => document.fonts.ready);

  for (let index = 0; index < FRAMES; index += 1) {
    const time = (index / FPS) * 1000;
    await page.evaluate((ms) => {
      for (const animation of document.getAnimations()) {
        animation.pause();
        animation.currentTime = ms;
      }
    }, time);
    await page.screenshot({
      path: path.join(dir, `frame-${String(index).padStart(3, "0")}.png`),
      clip: { x: 0, y: 0, width, height },
      type: "png",
    });
  }
  await page.close();
  return dir;
}

async function encodeWebp(frameDir, scene) {
  const out = path.join(assetDir, `readme-${scene}.webp`);
  await mkdir(assetDir, { recursive: true });
  await run(
    "ffmpeg",
    [
      "-y",
      "-loglevel",
      "error",
      "-framerate",
      String(FPS),
      "-i",
      path.join(frameDir, "frame-%03d.png"),
      "-loop",
      "0",
      "-c:v",
      "libwebp_anim",
      "-q:v",
      String(QUALITY),
      "-an",
      "-fps_mode",
      "passthrough",
      out,
    ],
    { maxBuffer: 1024 * 1024 * 32 },
  );
  const { size } = await stat(out);
  return { file: out, bytes: size };
}

/**
 * Prefer the locally installed Chrome so the script works without downloading a
 * Playwright browser build; fall back to the bundled one when Chrome is absent.
 */
async function launch(chromium) {
  try {
    return await chromium.launch({ channel: process.env.PW_CHANNEL ?? "chrome" });
  } catch {
    return await chromium.launch();
  }
}

async function main() {
  const only = process.argv.find((arg) => arg.startsWith("--only="))?.slice("--only=".length);
  const scenes = only ? [only] : Object.keys(SCENES);
  for (const scene of scenes) {
    if (!(scene in SCENES)) throw new Error(`unknown scene "${scene}"`);
  }

  const { chromium } = loadPlaywright();
  const browser = await launch(chromium);

  try {
    for (const scene of scenes) {
      const started = Date.now();
      const frames = await renderScene(browser, scene);
      try {
        const { file, bytes } = await encodeWebp(frames, scene);
        const { width, height } = SCENES[scene];
        console.log(
          `${scene}: ${FRAMES} frames · ${width}x${height} · ${(bytes / 1024).toFixed(0)} KB · ${(
            (Date.now() - started) / 1000
          ).toFixed(1)}s · ${path.relative(repoRoot, file)}`,
        );
      } finally {
        await rm(frames, { recursive: true, force: true });
      }
    }
  } finally {
    await browser.close();
  }
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : error);
  process.exit(1);
});
