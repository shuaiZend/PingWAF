/**
 * WAF rule action/mode compatibility — mirrors the server-side rejection of
 * `action=allow` with `mode=block` or `mode=off`.
 *
 * An allow rule is the exemption itself: `off` would remove it and let later
 * block rules run, which operators read as "the whitelist turned into a
 * block". An allow rule therefore only supports `monitor`.
 */

/** True when the pair would be rejected by the control plane. */
export function isAllowBlockConflict(action: string, mode: string): boolean {
  return action === 'allow' && (mode === 'block' || mode === 'off')
}

/**
 * Mode the dialog falls back to when the action switches to `allow` with a
 * mode it cannot use.
 */
export const ALLOW_FALLBACK_MODE = 'monitor'

/** Whether `mode` should be offered for the given rule action. */
export function isModeAllowedForAction(action: string, mode: string): boolean {
  return !isAllowBlockConflict(action, mode)
}
