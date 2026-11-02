// @ts-check
import { readdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "astro/config";

/**
 * Adds `<link rel="preload" as="style">` next to the stylesheet link that
 * Astro emits. The template cannot do this itself: the hashed filename only
 * exists after the build. Both links point at the same URL, so the browser
 * downloads the CSS once — the preload just raises its priority.
 */
function preloadStylesheets() {
  return {
    name: "pingwaf-preload-stylesheets",
    hooks: {
      /** @param {{ dir: URL; logger: { info: (message: string) => void } }} params */
      "astro:build:done": async ({ dir, logger }) => {
        const root = fileURLToPath(dir);
        let patched = 0;

        /** @param {string} current */
        const walk = async (current) => {
          for (const entry of await readdir(current, { withFileTypes: true })) {
            const full = path.join(current, entry.name);
            if (entry.isDirectory()) {
              await walk(full);
              continue;
            }
            if (!entry.name.endsWith(".html")) continue;

            const html = await readFile(full, "utf8");
            // `fetchpriority="auto"` is the implicit default — dropping it keeps
            // every img tag smaller without changing behaviour.
            const trimmed = html.replace(/(<img\b[^>]*?)\s+fetchpriority="auto"/g, "$1");
            const next = trimmed.replace(
              /<link rel="stylesheet" href="([^"]+)">/g,
              (match, href) =>
                html.includes(`rel="preload" as="style" href="${href}"`)
                  ? match
                  : `<link rel="preload" as="style" href="${href}" fetchpriority="high">${match}`,
            );
            if (next !== html) {
              await writeFile(full, next, "utf8");
              patched += 1;
            }
          }
        };

        await walk(root);
        logger.info(`preload hints injected into ${patched} page(s)`);
      },
    },
  };
}

/**
 * PingWAF official portal.
 *
 * Deployed as a Cloudflare Worker that serves static assets from `dist/`
 * (Workers Builds → GitHub integration → `wrangler deploy`). The project is
 * served at the domain root, so `base` stays `/`.
 *
 * `SITE_URL` must be set to the production hostname (`*.workers.dev` or the
 * custom domain) so canonical, hreflang and Open Graph URLs are absolute and
 * correct; it is read from the build environment, not hard-coded.
 *
 * Nothing here may depend on a server: the output is a pure static directory
 * — no SSR adapter, no runtime.
 */
const FALLBACK_SITE = "https://pingwaf.example.com";
const site = process.env.SITE_URL || FALLBACK_SITE;
const base = process.env.SITE_BASE ?? "/";

if (!process.env.SITE_URL) {
  console.warn(
    `[site] SITE_URL is not set — canonical/OG URLs fall back to ${FALLBACK_SITE}.`,
  );
}

export default defineConfig({
  site,
  base,
  output: "static",
  trailingSlash: "always",
  build: {
    format: "directory",
    inlineStylesheets: "auto",
  },
  prefetch: false,
  integrations: [preloadStylesheets()],
  devToolbar: { enabled: false },
  markdown: {
    shikiConfig: {
      themes: {
        light: "github-light",
        dark: "github-dark-dimmed",
      },
      // Emit `--shiki-light` / `--shiki-dark` CSS variables instead of inline
      // colours so the theme toggle in `global.css` can switch them.
      defaultColor: false,
      wrap: false,
    },
  },
  vite: {
    build: {
      // Screenshots are hashed and immutable; keep the HTML/CSS budget small.
      assetsInlineLimit: 2048,
    },
  },
});
