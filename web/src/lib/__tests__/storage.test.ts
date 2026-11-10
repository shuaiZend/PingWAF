import { afterEach, describe, expect, it, vi } from 'vitest'
import { webStorage } from '../storage'

// The store modules read `localStorage` at import time. Node (>=22) defines the
// global without a usable `Storage`, which used to crash every test that
// imported the API client, so the guard itself is worth pinning down.
describe('webStorage', () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('returns the global when it is a real Storage', () => {
    const fake = { getItem: () => null, setItem: () => undefined, removeItem: () => undefined }
    vi.stubGlobal('localStorage', fake)
    expect(webStorage()).toBe(fake)
    vi.unstubAllGlobals()
  })

  it('returns null when the global exists but cannot read', () => {
    vi.stubGlobal('localStorage', {})
    expect(webStorage()).toBeNull()
    vi.unstubAllGlobals()
  })

  it('returns null when the global is missing', () => {
    vi.stubGlobal('localStorage', undefined)
    expect(webStorage()).toBeNull()
    vi.unstubAllGlobals()
  })

  it('returns null when accessing the global throws', () => {
    vi.stubGlobal('localStorage', {
      get getItem(): never {
        throw new Error('denied')
      },
    })
    expect(webStorage()).toBeNull()
    vi.unstubAllGlobals()
  })
})
