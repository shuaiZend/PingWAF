import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'

import type { ImageOptimization, UpdateCacheSettingsRequest } from '@/api/types'

const read = (path: string) => readFileSync(resolve(process.cwd(), path), 'utf8')

/**
 * Image optimization contract: the policy is per site, the switch lives on the
 * caching page, and the three pieces that have to agree — the console type, the
 * form defaults and the page — stay in step. The server side (validation and
 * the plugin conf) is pinned by Rust tests; this pins the wire shape the
 * console sends.
 */
describe('image optimization contract', () => {
  it('the policy carries the switch, the formats and the encoder knobs', () => {
    const policy: ImageOptimization = {
      enabled: true,
      formats: ['avif', 'webp'],
      png_quality: 90,
      jpeg_quality: 80,
      avif_quality: 75,
      avif_speed: 3,
    }
    const request: UpdateCacheSettingsRequest = { image_optimization: policy }
    expect(request.image_optimization?.formats).toEqual(['avif', 'webp'])
    // `null` is the explicit "stop optimizing" value, distinct from absent
    // (leave the stored policy alone).
    const cleared: UpdateCacheSettingsRequest = { image_optimization: null }
    expect(cleared.image_optimization).toBeNull()
  })

  it('the caching page renders the switch and the encoder inputs', () => {
    const page = read('src/pages/sites/CachingPage.tsx')
    expect(page).toContain('imageOptimTitle')
    expect(page).toContain('imageOptimSwitch')
    expect(page).toContain('imageOptimFormatsLabel')
    expect(page).toContain('saveImageOptim')
    // The formats are the two the plugin can produce.
    expect(page).toContain("key: 'webp'")
    expect(page).toContain("key: 'avif'")
  })

  it('the console defaults match the plugin defaults', () => {
    // `ImageOptimization::default()` on the server: webp+avif, 90/80/75, speed
    // 3. The page seeds an unsaved policy with the same values, so a drift
    // would show the operator numbers the edge does not use.
    const page = read('src/pages/sites/CachingPage.tsx')
    expect(page).toContain('png_quality: 90')
    expect(page).toContain('jpeg_quality: 80')
    expect(page).toContain('avif_quality: 75')
    expect(page).toContain('avif_speed: 3')
  })
})