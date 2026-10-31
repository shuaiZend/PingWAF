#!/usr/bin/env node
/**
 * Render the terminal screenshots used by the install page.
 *
 * The fixtures below are transcripts of what `install.sh` actually prints
 * (see the `prompt_line` / `print_plan_summary` / success banner sections), so
 * the page can show a realistic walkthrough without a Linux box in the loop.
 * They are rendered through Playwright and written to
 * `src/assets/screenshots/install-*.png`.
 *
 *   npm run capture:install
 *
 * Keep the text in sync with install.sh when the prompts or the summary change.
 */
import { existsSync, mkdirSync, readdirSync } from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const siteRoot = path.resolve(here, "..");
const repoRoot = path.resolve(siteRoot, "..");
const outDir = path.join(siteRoot, "src", "assets", "screenshots");

const VIEWPORT = { width: 1280, height: 820 };

function loadPlaywright() {
  const candidates = [
    path.join(repoRoot, "e2e", "package.json"),
    path.join(repoRoot, "web", "package.json"),
    path.join(siteRoot, "package.json"),
  ];
  for (const candidate of candidates) {
    try {
      const require = createRequire(candidate);
      return require("@playwright/test");
    } catch (error) {
      // try the next resolution root
    }
  }
  throw new Error(
    "Playwright not found. Install it (npm i -D @playwright/test) or reuse e2e/node_modules.",
  );
}

/** Reuse any Chromium already present in the Playwright cache. */
function findCachedChromium() {
  const cacheRoot = path.join(process.env.HOME ?? "", "Library", "Caches", "ms-playwright");
  if (!existsSync(cacheRoot)) return undefined;
  const candidates = [];
  for (const entry of readdirSync(cacheRoot)) {
    if (entry.startsWith("chromium_headless_shell-")) {
      candidates.push(
        path.join(cacheRoot, entry, "chrome-headless-shell-mac-arm64", "chrome-headless-shell"),
        path.join(cacheRoot, entry, "chrome-headless-shell-linux64", "chrome-headless-shell"),
      );
    } else if (entry.startsWith("chromium-")) {
      candidates.push(
        path.join(
          cacheRoot,
          entry,
          "chrome-mac-arm64",
          "Google Chrome for Testing.app",
          "Contents",
          "MacOS",
          "Google Chrome for Testing",
        ),
        path.join(cacheRoot, entry, "chrome-linux64", "chrome"),
      );
    }
  }
  return candidates.find((candidate) => existsSync(candidate));
}

const escapes = {
  "&": "&amp;",
  "<": "&lt;",
  ">": "&gt;",
};

function line(text, tone = "") {
  const escaped = text.replace(/[&<>]/g, (char) => escapes[char]);
  return `<div class="line ${tone}">${escaped || "&nbsp;"}</div>`;
}

function terminal({ title, lines }) {
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<style>
  :root { color-scheme: dark; }
  * { box-sizing: border-box; }
  body {
    margin: 0;
    width: ${VIEWPORT.width}px;
    padding: 40px 50px;
    background: #f2f2f3;
    font-family: "SFMono-Regular", "JetBrains Mono", "Menlo", "Consolas", monospace;
  }
  .window {
    width: 1180px;
    border-radius: 14px;
    overflow: hidden;
    background: #1d1d1d;
    box-shadow: 0 24px 60px rgba(0, 0, 0, 0.28);
    border: 1px solid #2c2c2c;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 16px;
    background: #262626;
    border-bottom: 1px solid #303030;
  }
  .dot { width: 11px; height: 11px; border-radius: 50%; }
  .dot:nth-child(1) { background: #ff5f57; }
  .dot:nth-child(2) { background: #febc2e; }
  .dot:nth-child(3) { background: #28c840; }
  .title {
    margin-left: 10px;
    font-size: 12.5px;
    color: #9a9a9a;
    letter-spacing: 0.02em;
  }
  .body {
    padding: 22px 26px 26px;
    font-size: 14px;
    line-height: 1.62;
    color: #e8e8e8;
    white-space: pre-wrap;
  }
  .line { min-height: 22px; }
  .info { color: #7fb4e8; }
  .ok { color: #57c98a; }
  .warn { color: #ffb224; }
  .prompt { color: #f6821f; }
  .dim { color: #9a9a9a; }
  .rule { color: #57c98a; letter-spacing: 0.06em; }
  .bold { color: #ffffff; font-weight: 600; }
</style>
</head>
<body>
  <div class="window">
    <div class="bar">
      <span class="dot"></span><span class="dot"></span><span class="dot"></span>
      <span class="title">${title}</span>
    </div>
    <div class="body">
${lines.join("\n")}
    </div>
  </div>
</body>
</html>`;
}

const interactive = terminal({
  title: "bash install.sh — interactive setup",
  lines: [
    line("PingWAF interactive installer", "bold"),
    line("Press Enter to accept the value in brackets.", "dim"),
    line(""),
    line("Deployment mode:"),
    line("  1) all-in-one  control plane and agent in one process (needs PostgreSQL)"),
    line("  2) server      control plane only"),
    line("  3) agent       data-plane node, connects to a control plane"),
    line(""),
    line("? Mode [1-3 or name] [all-in-one]: 2", "prompt"),
    line("? Administrator e-mail [admin@pingwaf.local]: ops@example.com", "prompt"),
    line("? Administrator password (empty keeps the default, changed at first sign-in):", "prompt"),
    line("? Repeat administrator password:", "prompt"),
    line("? PostgreSQL URL [postgres://pingwaf:pingwaf@localhost:5432/pingwaf]:", "prompt"),
    line("  postgres://pingwaf:***@db.internal:5432/pingwaf", "dim"),
    line("? Dashboard certificate names (comma separated, empty for none): waf.example.com", "prompt"),
    line("? Version (empty or 'latest' = latest release) [latest]:", "prompt"),
    line(""),
    line("Installation plan", "bold"),
    line("  mode:        server"),
    line("  version:     latest release"),
    line("  platform:    linux/amd64"),
    line("  binary:      /usr/local/bin/pingwaf"),
    line("  config:      /etc/pingwaf/pingwaf.toml"),
    line("  data:        /var/lib/pingwaf"),
    line("  systemd:     yes (User=pingwaf, CAP_NET_BIND_SERVICE for 80/443)"),
    line("  database:    postgres://pingwaf:***@db.internal:5432/pingwaf"),
    line("  admin:       ops@example.com (password: custom)"),
    line("  tls sans:    waf.example.com"),
    line(""),
    line("? Continue? [Y/n] [y]:", "prompt"),
  ],
});

const success = terminal({
  title: "bash install.sh — unattended one-liner",
  lines: [
    line("> Fetching latest release version...", "info"),
    line("> Installing PingWAF v0.33.0", "info"),
    line("> Downloading pingwaf-linux-amd64.tar.xz...", "info"),
    line("✓ Binary installed: /usr/local/bin/pingwaf", "ok"),
    line("> Creating directories...", "info"),
    line("✓ Created /etc/pingwaf and /var/lib/pingwaf", "ok"),
    line("> Configuration written to /etc/pingwaf/pingwaf.toml", "ok"),
    line("> Creating systemd service...", "info"),
    line("✓ Systemd service installed: pingwaf.service", "ok"),
    line("> Checking PostgreSQL availability...", "info"),
    line("✓ PostgreSQL connection successful.", "ok"),
    line(""),
    line("═══════════════════════════════════════════════════════════", "rule"),
    line("  PingWAF v0.33.0 installed successfully!", "bold"),
    line("═══════════════════════════════════════════════════════════", "rule"),
    line(""),
    line("  Binary:    /usr/local/bin/pingwaf"),
    line("  Config:    /etc/pingwaf/pingwaf.toml"),
    line("  Data:      /var/lib/pingwaf"),
    line("  Service:   systemctl {start|stop|restart|status} pingwaf"),
    line(""),
    line("  Ports 80/443 are bound by User=pingwaf through the unit's"),
    line("  CAP_NET_BIND_SERVICE capability. Verify with:"),
    line("    systemctl show pingwaf -p AmbientCapabilities", "dim"),
    line(""),
    line("  Next steps:", "bold"),
    line("    1. Ensure PostgreSQL is running and accessible"),
    line("    2. Start the service: sudo systemctl enable --now pingwaf", "dim"),
  ],
});

const compose = terminal({
  title: "docker compose up -d — first start",
  lines: [
    line("$ docker compose up -d", "prompt"),
    line("[+] Running 4/4", "dim"),
    line(" ✔ Network pingwaf-net                Created", "ok"),
    line(" ✔ Volume \"pingwaf_pingwaf-data\"      Created", "ok"),
    line(" ✔ Container pingwaf-postgres         Healthy", "ok"),
    line(" ✔ Container pingwaf                  Healthy", "ok"),
    line(""),
    line("> Waiting for the control plane to accept logins...", "info"),
    line("✓ Dashboard ready: https://localhost:9080", "ok"),
    line("  (self-signed certificate — accept the browser warning, then upload a"),
    line("   real certificate under Settings → Control plane HTTPS)", "dim"),
    line(""),
    line("  Next steps:", "bold"),
    line("    1. Log in with the credentials from your .env file"),
    line("       (default: admin@pingwaf.local / pingwaf123)", "dim"),
    line("    2. Change the administrator password at first sign-in", "dim"),
    line("    3. Add your first site under Sites", "dim"),
    line(""),
    line("  Upgrades: docker compose pull && docker compose up -d", "info"),
  ],
});

async function main() {
  mkdirSync(outDir, { recursive: true });
  const { chromium } = loadPlaywright();
  const browser = await chromium.launch({ headless: true, executablePath: findCachedChromium() });
  const context = await browser.newContext({
    viewport: VIEWPORT,
    deviceScaleFactor: 2,
    colorScheme: "dark",
    reducedMotion: "reduce",
    locale: "en-US",
  });
  const page = await context.newPage();

  const shots = [
    ["install-interactive.png", interactive],
    ["install-success.png", success],
    ["install-compose.png", compose],
  ];

  for (const [file, html] of shots) {
    await page.setContent(html, { waitUntil: "load" });
    // fullPage keeps the whole transcript: the fixture grows with its content
    // instead of being clipped to a fixed viewport.
    await page.screenshot({
      path: path.join(outDir, file),
      type: "png",
      fullPage: true,
    });
    console.log(`captured ${path.relative(siteRoot, path.join(outDir, file))}`);
  }

  await browser.close();
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
