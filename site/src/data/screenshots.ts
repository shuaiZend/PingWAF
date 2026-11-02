import type { ImageMetadata } from "astro";
import type { ScreenshotKey } from "../content/schema";

import configVersions from "../assets/screenshots/configVersions.png";
import installInteractive from "../assets/screenshots/install-interactive.png";
import installSuccess from "../assets/screenshots/install-success.png";
import installCompose from "../assets/screenshots/install-compose.png";
import login from "../assets/screenshots/login.png";
import sites from "../assets/screenshots/sites.png";

/**
 * Screenshot manifest — the only place that knows a screenshot's file.
 *
 * Files are produced by `npm run capture` (see `scripts/capture-screenshots.mjs`)
 * and imported here so Astro can emit AVIF/WebP variants at build time.
 * Content files reference screenshots by key only.
 */
export const SCREENSHOT_IMAGES: Record<ScreenshotKey, ImageMetadata> = {
  sites,
  login,
  configVersions,
  installInteractive,
  installSuccess,
  installCompose,
};
