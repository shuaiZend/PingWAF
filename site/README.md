# PingWAF official site

Static, multilingual portal for PingWAF, built with [Astro](https://astro.build)
and deployed to **Cloudflare** as the Worker `pingwaf`, serving static assets
from `dist/` (Workers Builds, GitHub integration).

Everything here is output as plain HTML/CSS with a few kilobytes of inline
JavaScript — no server runtime, no API calls, no external fonts. The dashboard
screenshots are real captures of `web/dist` and are optimised at build time.

## Layout

| Path | Purpose |
| --- | --- |
| `astro.config.mjs` | Static output, `site`/`base` env hooks, Shiki themes |
| `wrangler.toml` | Worker name and `[assets]` config (`dist`, `404-page` handling) |
| `public/_headers` | Security headers plus immutable caching for `/_astro/*` |
| `public/_redirects` | Canonical entry point (`/index.html` → `/`) |
| `src/pages/` | Routes: English at the root, `[locale]/` mirrors for zh/es/fr/ja |
| `src/pages/404.astro` | Not-found page, served via `not_found_handling = "404-page"` |
| `src/layouts/BaseLayout.astro` | `<head>`, hreflang/canonical/OG, theme + language scripts, header/footer |
| `src/layouts/DocsLayout.astro` | Three-column documentation shell with sidebar and page TOC |
| `src/components/` | Header, footer, screenshot frame, code block, section primitives |
| `src/components/home/HeroVisual.astro` | Inline SVG control-plane animation for the homepage hero — zero JS, CSS keyframes only, follows `html[data-theme]` and renders a static frame under `prefers-reduced-motion`. The component has no `<style>` block; its styles live in the `/* Hero visual (homepage) */` section of `src/styles/global.css` |
| `src/components/scenes/` | Inline SVG console scenes for the homepage showcase, the features page and the quickstart source path — `SceneHost.astro` dispatches on a scene id to one of seven components (`SceneWaf`, `SceneBot`, `SceneAccess`, `SceneCaching`, `SceneTls`, `SceneLogs`, `SceneAgents`) over a shared `SceneBackdrop.astro`. Styles live in the `/* Console scenes */` section of `src/styles/global.css` |
| `src/content/<locale>/*.ts` | Typed marketing copy: `intro`, `features`, `quickstart`, `docs` |
| `src/content/docs/<locale>/*.md` | Long-form documentation (Content Collection) |
| `src/content/schema.ts` | **The content contract** — every locale implements these shapes |
| `src/content/index.ts` | Discovers locale modules at build time (no shared registry file) |
| `src/i18n/` | Locale list, UI strings, path helpers, detection/theme scripts |
| `src/i18n/seo.ts` | Per-page keyword lists (`SEO_KEYWORDS`, five locales × six page kinds) and the route → page kind map (`pageKindFor`) used by `BaseLayout` |
| `src/styles/` | Design tokens copied from the dashboard plus portal layout |
| `src/assets/screenshots/` | Console screenshots, consumed through `astro:assets` |
| `src/data/screenshots.ts` | The only place that maps a screenshot key to a file |
| `scripts/capture-screenshots.mjs` | Serves `web/dist`, stubs `/api/v1`, captures the console |
| `scripts/check-content.ts` | Five-locale key/shape parity, links, docs and screenshot checks |
| `scripts/check-links.mjs` | Post-build internal link and asset audit |

## Local development

```bash
cd site
npm ci
npm run dev          # http://localhost:4321/
npm run verify       # content contract + type check + build + link audit
```

`npm run verify` is the gate CI runs; keep it green before opening a PR.

## Content rules

- **English is the source of truth.** Edit `src/content/en/*.ts` first, then mirror
  the change in `zh`, `es`, `fr` and `ja`. `npm run check:content` fails when a key
  is missing, an array length differs, a placeholder (`TODO:`) survives, or a
  screenshot key/link points nowhere.
- Facts must be verifiable in the repository (`README*.md`, `docs/**`,
  `pingwaf-*/src`). Do not invent defaults, ports or endpoints.
- Keep commands, code samples, URLs, screenshot keys and `slug`/`id` values
  untranslated — the checker treats them as machine identifiers.
- Showcase imagery comes in two shapes: `Shot` (a key from `SCREENSHOT_KEYS`,
  resolved to a captured PNG) and `Scene` (an id from `SCENE_IDS`, rendered by
  `SceneHost.astro`). Use the shape the slot declares and do not mix them.
  `check:content` validates scene ids against the `SCENE_IDS` whitelist,
  requires non-empty `alt` and `caption`, and requires the features page
  groups to cover all seven scene ids.
- Long-form pages live in `src/content/docs/<locale>/<slug>.md` with
  `title`, `description` and `minutes` frontmatter. Slugs are defined once in
  `src/content/schema.ts` (`DOC_SLUGS`), which also drives routes and the sidebar.
- When a new page kind is introduced, add its keyword list to every locale in
  `SEO_KEYWORDS` (`src/i18n/seo.ts`). Routes that `pageKindFor` does not
  recognise fall back to the `home` keywords.
- Internal links are locale-less paths (`/features/`, `/docs/api/`) and are
  resolved with `href()` / `resolveLink()` so the deployment base and locale
  prefix are never dropped. Absolute URLs pass through untouched.

## SEO metadata

Titles and descriptions come from `src/content/<locale>/*.ts`; keywords come
from `src/i18n/seo.ts`. `BaseLayout` derives the page kind from the route with
`pageKindFor(path)` and takes the matching list from `SEO_KEYWORDS` (five
locales × six page kinds), so a page gets keywords without passing any props —
and a page can still override them with the `keywords` prop. `BaseLayout` also
emits `meta robots` (`index,follow,max-image-preview:large`; `noindex` pages
keep `noindex,nofollow`), `og:image:width/height/type`, `twitter:image:alt`
and `meta author`.

Structured data is one `<script type="application/ld+json">` per page with a
`@graph` that always contains `Organization`, `WebSite` and `WebPage`, linked
by `@id`. Page-level nodes are appended by the components themselves: the
homepage (`HomePage.astro`) adds a `SoftwareApplication`, and documentation
pages (`DocsLayout.astro`) add a `TechArticle` plus a `BreadcrumbList` that is
built from the same visible breadcrumb trail shown in the docs shell.

## Adding a language

1. Add the locale to `LOCALES` and `LOCALE_META` in `src/i18n/config.ts`
   (native label, `htmlLang`, Open Graph locale, detection prefixes).
2. Add `src/content/<locale>/{intro,features,quickstart,docs}.ts`, each
   `satisfies` the type from `src/content/schema.ts` and exporting it as default.
3. Translate `src/ui.ts` (nav, buttons, footer, accessibility labels).
4. Add `src/content/docs/<locale>/{architecture,deployment,waf-engine,api}.md`.
5. Run `npm run verify`. `hreflang`, canonical URLs and the switcher are generated
   from the locale list — no other file needs editing.

## Screenshots

Most console imagery is no longer a screenshot. The homepage showcase, the
features page groups and the quickstart "source" path all render inline SVG
scenes: `SceneHost.astro` dispatches on a stable id to one of the seven
components in `src/components/scenes/` (`SceneWaf`, `SceneBot`,
`SceneAccess`, `SceneCaching`, `SceneTls`, `SceneLogs`, `SceneAgents`), laid
out over the shared `SceneBackdrop.astro` and styled by the
`/* Console scenes */` section of `src/styles/global.css`. The content
contract exposes `SCENE_IDS` (`waf`, `bot`, `access`, `caching`, `tls`,
`logs`, `agents`) and `Scene { id, caption, alt }`, so the same component is
reused across pages and locales while the copy stays localised. The homepage
hero visual (`HeroVisual.astro`) is a separate inline SVG animation. Only six
screenshot keys remain in `SCREENSHOT_KEYS` — `sites`, `login`,
`configVersions`, `installInteractive`, `installSuccess` and
`installCompose` — and they are still produced by:

```bash
npm run capture
```

The script serves the built dashboard (`web/dist`, build it with
`make build-web` first), fulfils every `/api/v1/**` request from
`scripts/fixtures.mjs`, logs in by injecting the session keys the console uses,
and writes 1440×900 @2x PNGs into `src/assets/screenshots/`. Recipes live in
`SHOTS` at the top of the script, now only three — `sites`, `login` and
`configVersions`; all demo data is `example.com` plus RFC 5737 ranges.

For a real control plane:

```bash
E2E_ADMIN_EMAIL=admin@pingwaf.local E2E_ADMIN_PASSWORD='…' \
  npm run capture -- --live --base=https://localhost:9080
```

Screenshots are committed, so the site build never needs a browser. They are
imported through `src/data/screenshots.ts` and emitted as AVIF/WebP at build
time — never put them in `public/`.

## Build

```bash
npm run build      # → site/dist
npm run preview    # serve dist locally at http://localhost:4321/
```

`dist/` is a self-contained static bundle: HTML, hashed `/_astro/*` assets,
`404.html`, `_headers` and `_redirects`. It is served from the domain root, so
`base` is `/`; set `SITE_BASE=/something/` when auditing a sub-path deployment.

The inline `pingwaf-preload-stylesheets` integration in `astro.config.mjs`
runs at `astro:build:done` and rewrites every generated HTML file: a
`<link rel="preload" as="style" … fetchpriority="high">` is inserted before
each stylesheet link so the browser starts fetching CSS before parsing reaches
it, and the redundant `fetchpriority="auto"` is stripped from `<img>` tags.
The hook is idempotent — a stylesheet that already has a matching preload is
left untouched — so re-running the build (or a manual post-processing pass)
cannot duplicate tags.

## Deployment

Production is the Cloudflare **Worker** `pingwaf`, connected to this GitHub
repository through Workers Builds. Cloudflare builds and deploys on its own —
no GitHub Actions deployment runs in the normal flow.

The Worker has no script: `wrangler.toml` declares `[assets] directory =
"./dist"`, so the upload is pure static hosting, and `_headers` / `_redirects`
inside `dist` are honoured by Workers static assets.

### Build settings (Dashboard → Workers & Pages → pingwaf → Settings → Build)

| Setting | Value |
| --- | --- |
| Git branch | `main` (production branch) |
| Root directory | `site` |
| Build command | `npm run build` |
| Deploy command | `npx wrangler deploy` |
| Build variable | `SITE_URL=https://<production host>`, set for Build and Preview |

Three details break the deploy if they drift:

- **Deploy command must be the Workers one.** `npx wrangler pages deploy …`
  calls the Pages API and fails with `Authentication error [code: 10000]`,
  because the build environment's token is scoped to Workers, not Pages.
- **Never put `pages_build_output_dir` back in `wrangler.toml`.** Wrangler then
  treats the config as a Pages project and `wrangler deploy` stops with
  `Missing entry-point to Worker script or to assets directory`.
- **`name` must equal the Cloudflare project name** (`pingwaf`).

Pushes to `main` deploy to production; pushes to other branches produce preview
versions, which is a cheap way to review a locale or layout change.

### Optional: manual deploy from GitHub Actions

`.github/workflows/cloudflare-deploy.yml` runs only on `workflow_dispatch`, so
it never races Workers Builds. Use it when the Cloudflare connection is
unavailable or an older commit must be rebuilt; it needs two secrets:

| Secret | Value |
| --- | --- |
| `CLOUDFLARE_API_TOKEN` | API token with **Workers Scripts: Edit** on the account |
| `CLOUDFLARE_ACCOUNT_ID` | Account id (visible in the dashboard URL) |

### Manual deploy from a workstation

```bash
cd site
npm run build
npx wrangler deploy
```

### Custom domain and `SITE_URL`

Add the hostname under **Worker → Settings → Domains & Routes → Custom domain**
(Cloudflare issues the certificate), then set the `SITE_URL` build variable to
that hostname and redeploy. Without `SITE_URL`, canonical, hreflang and Open
Graph URLs fall back to the placeholder `https://pingwaf.example.com` and the
build prints a warning:

```bash
SITE_URL=https://pingwaf.example.com npm run build   # same result locally
```

## Troubleshooting

| Symptom | Cause |
| --- | --- |
| `check:links` reports "ignores the / base path" | A raw `href="/…"` was used instead of `href()` / `resolveLink()` |
| `check:content` reports an unexpected key | A locale file drifted from `src/content/schema.ts`; change the schema first |
| Canonical URLs point at `pingwaf.example.com` | `SITE_URL` build variable is not set |
| Screenshots look stale | Re-run `npm run capture` after `make build-web` |
| Deploy fails with `Authentication error [code: 10000]` | Deploy command is `wrangler pages deploy`; switch it to `wrangler deploy` |
| Deploy fails with `Missing entry-point to Worker script` | `wrangler.toml` still contains `pages_build_output_dir` |
| Deploy fails with 401 (manual workflow) | `CLOUDFLARE_API_TOKEN` lacks **Workers Scripts: Edit** |
| Cloudflare rejects the build with a name mismatch | `wrangler.toml` `name` differs from the Worker name |
