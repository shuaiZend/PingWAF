/**
 * WAF rule action/mode compatibility — mirrors the server-side rejection of
 * `action=allow` + `mode=block` (400, "action 'allow' cannot use mode 'block'").
 */

/** True when the pair would be rejected by the control plane. */
export function isAllowBlockConflict(action: string, mode: string): boolean {
  return action === 'allow' && mode === 'block'
}

/**
 * Mode the dialog falls back to when the action switches to `allow` while
 * `block` is selected: `monitor` keeps the rule visible without blocking.
 */
export const ALLOW_FALLBACK_MODE = 'monitor'

/** Whether `mode` should be offered for the given rule action. */
export function isModeAllowedForAction(action: string, mode: string): boolean {
  return !isAllowBlockConflict(action, mode)
}
