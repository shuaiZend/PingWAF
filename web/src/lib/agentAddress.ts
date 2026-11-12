/**
 * Address classification helpers for the agents list.
 *
 * Agents self-report an `ip_address`, but that value may be a container-bridge
 * or RFC1918 address that is unreachable from outside. The control plane also
 * observes the connection's actual source (`peer_ip`). These helpers decide
 * when the UI should explain the difference between the two.
 */

/** [base octets, mask octets] — RFC1918 + loopback + link-local. */
const IPV4_PRIVATE_RANGES: Array<{
  base: [number, number, number, number]
  mask: [number, number, number, number]
}> = [
  { base: [10, 0, 0, 0], mask: [255, 0, 0, 0] }, // 10/8
  { base: [172, 16, 0, 0], mask: [255, 240, 0, 0] }, // 172.16/12
  { base: [192, 168, 0, 0], mask: [255, 255, 0, 0] }, // 192.168/16
  { base: [127, 0, 0, 0], mask: [255, 0, 0, 0] }, // 127/8
  { base: [169, 254, 0, 0], mask: [255, 255, 0, 0] }, // 169.254/16
]

/**
 * Parse a dotted-quad IPv4 address. Returns null for anything that is not
 * exactly four decimal octets — no ranges, no partial octets, no overflow.
 */
function parseIPv4(value: string): [number, number, number, number] | null {
  const parts = value.split('.')
  if (parts.length !== 4) return null
  const octets: number[] = []
  for (const part of parts) {
    if (!/^\d{1,3}$/.test(part)) return null
    const n = Number(part)
    if (n > 255) return null
    octets.push(n)
  }
  return [octets[0], octets[1], octets[2], octets[3]]
}

const inRange = (
  addr: [number, number, number, number],
  base: [number, number, number, number],
  mask: [number, number, number, number],
): boolean => mask.every((m, i) => (addr[i] & m) === (base[i] & m))

/** Whether a bare IPv4 address falls in one of the private / special ranges. */
function isPrivateIPv4(addr: [number, number, number, number]): boolean {
  if (addr.every((o) => o === 0)) return true // 0.0.0.0
  return IPV4_PRIVATE_RANGES.some(({ base, mask }) => inRange(addr, base, mask))
}

/**
 * True when `value` is a private / non-globally-routable address:
 * IPv4 RFC1918 + loopback + link-local + 0.0.0.0, IPv6 ::1 / fc00::/7 /
 * fe80::/10, and IPv4-mapped forms (`::ffff:10.0.0.1`). Unparsable input
 * (including hostnames) returns false — never throws.
 */
export function isPrivateAddress(value: string): boolean {
  if (typeof value !== 'string') return false
  const trimmed = value.trim()
  if (trimmed === '') return false

  // IPv4-mapped IPv6: ::ffff:a.b.c.d (also tolerate the bare IPv4 tail).
  const mapped = trimmed.match(/^::ffff:(\d{1,3}(?:\.\d{1,3}){3})$/i)
  if (mapped) {
    const addr = parseIPv4(mapped[1])
    return addr !== null && isPrivateIPv4(addr)
  }

  // Bare IPv4.
  const v4 = parseIPv4(trimmed)
  if (v4) return isPrivateIPv4(v4)

  // IPv6: lowercase, strip zone index (%eth0).
  const v6 = trimmed.toLowerCase().split('%')[0]
  if (!v6.includes(':') || v6.includes('..')) return false

  // Expand :: shorthand into 8 hextets.
  let head: string[]
  let tail: string[]
  const dbl = v6.indexOf('::')
  if (dbl !== -1) {
    if (v6.indexOf('::', dbl + 1) !== -1) return false // more than one ::
    head = dbl === 0 ? [] : v6.slice(0, dbl).split(':')
    tail = dbl + 2 === v6.length ? [] : v6.slice(dbl + 2).split(':')
  } else {
    head = v6.split(':')
    tail = []
  }
  if (head.length + tail.length > 8) return false
  const groups = [...head, ...Array(8 - head.length - tail.length).fill('0'), ...tail]
  if (groups.length !== 8) return false
  if (!groups.every((g) => /^[0-9a-f]{1,4}$/.test(g))) return false
  const nums = groups.map((g) => parseInt(g, 16))

  if ((nums[0] & 0xfe00) === 0xfc00) return true // fc00::/7
  if ((nums[0] & 0xffc0) === 0xfe80) return true // fe80::/10
  if (nums.slice(0, 7).every((n) => n === 0) && (nums[7] === 1 || nums[7] === 0)) return true // ::1, ::
  return false
}

export type PeerAddressHint = 'private' | 'mismatch' | null

/**
 * Decide what the agents list should explain about the two addresses:
 * - 'private'  — self-reported address is a private/unroutable one; peer_ip is
 *                the address actually reachable from the control plane.
 * - 'mismatch' — the two differ but the self-reported one looks public.
 * - null       — same, or either side is missing.
 */
export function peerAddressHint(agent: {
  ip_address?: string | null
  peer_ip?: string | null
}): PeerAddressHint {
  const self = agent.ip_address?.trim() || null
  const peer = agent.peer_ip?.trim() || null
  if (!self || !peer || self === peer) return null
  return isPrivateAddress(self) ? 'private' : 'mismatch'
}
