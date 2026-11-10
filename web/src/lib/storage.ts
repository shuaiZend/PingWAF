/**
 * `localStorage` access that tolerates environments where the global exists but
 * is not a usable `Storage` (Node's experimental Web Storage, SSR shims, or a
 * privacy mode that throws on access).
 *
 * `typeof localStorage === 'undefined'` is not enough there: the global is
 * defined, so the guard passes and `getItem` turns out not to be a function.
 */
export function webStorage(): Storage | null {
  try {
    const storage = globalThis.localStorage
    return storage && typeof storage.getItem === 'function' ? storage : null
  } catch {
    return null
  }
}
