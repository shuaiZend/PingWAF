import { describe, expect, it } from 'vitest'
import { isPrivateAddress, peerAddressHint } from '@/lib/agentAddress'

describe('isPrivateAddress — IPv4', () => {
  it.each([
    ['10.0.0.1', true],
    ['10.255.255.255', true],
    ['172.16.0.1', true],
    ['172.31.255.255', true], // top of 172.16/12
    ['172.15.0.1', false], // just below the range
    ['172.32.0.1', false], // just above the range
    ['192.168.1.10', true],
    ['192.169.1.1', false],
    ['127.0.0.1', true],
    ['127.200.1.1', true],
    ['169.254.10.5', true],
    ['0.0.0.0', true],
    ['47.88.23.193', false],
    ['8.8.8.8', false],
  ])('%s -> %s', (input, expected) => {
    expect(isPrivateAddress(input)).toBe(expected)
  })
})

describe('isPrivateAddress — IPv6', () => {
  it.each([
    ['::1', true], // loopback
    ['::', true], // unspecified
    ['fc00::1', true], // ULA
    ['fdab:1234:5678::9', true], // ULA (locally assigned half)
    ['fbff::1', false], // below fc00::/7
    ['fe00::1', false], // below fe80::/10
    ['fe80::1', true], // link-local
    ['febf:ffff::1', true], // top of fe80::/10
    ['fec0::1', false], // just above
    ['2001:db8::1', false], // global unicast
  ])('%s -> %s', (input, expected) => {
    expect(isPrivateAddress(input)).toBe(expected)
  })
})

describe('isPrivateAddress — IPv4-mapped IPv6', () => {
  it.each([
    ['::ffff:10.0.0.5', true],
    ['::ffff:192.168.0.9', true],
    ['::ffff:127.0.0.1', true],
    ['::ffff:47.88.23.193', false],
    ['::ffff:8.8.8.8', false],
  ])('%s -> %s', (input, expected) => {
    expect(isPrivateAddress(input)).toBe(expected)
  })
})

describe('isPrivateAddress — invalid input', () => {
  it.each([
    '',
    '   ',
    'not-an-ip',
    'agent.example.com',
    '999.1.1.1',
    '10.0.0',
    '10.0.0.256',
    '::g::',
    '1::2::3',
    'fe80::1%eth0', // zone index still classifies, but no crash
  ])('%j does not throw', (input) => {
    expect(() => isPrivateAddress(input)).not.toThrow()
  })

  it('returns false for unparsable values', () => {
    expect(isPrivateAddress('agent.example.com')).toBe(false)
    expect(isPrivateAddress('999.1.1.1')).toBe(false)
    expect(isPrivateAddress('1::2::3')).toBe(false)
  })
})

describe('peerAddressHint', () => {
  it("returns 'private' when the self-reported address is a private one", () => {
    expect(
      peerAddressHint({ ip_address: '172.17.24.30', peer_ip: '47.88.23.193' }),
    ).toBe('private')
    expect(peerAddressHint({ ip_address: '::ffff:10.0.0.1', peer_ip: '8.8.8.8' })).toBe(
      'private',
    )
  })

  it("returns 'mismatch' when both are present, differ, and the self-reported one is public", () => {
    expect(
      peerAddressHint({ ip_address: '47.88.23.193', peer_ip: '51.10.0.8' }),
    ).toBe('mismatch')
  })

  it('returns null when addresses match, or either side is missing/blank', () => {
    expect(peerAddressHint({ ip_address: '10.0.0.1', peer_ip: '10.0.0.1' })).toBeNull()
    expect(peerAddressHint({ ip_address: '10.0.0.1', peer_ip: null })).toBeNull()
    expect(peerAddressHint({ ip_address: null, peer_ip: '8.8.8.8' })).toBeNull()
    expect(peerAddressHint({ ip_address: '', peer_ip: '8.8.8.8' })).toBeNull()
    expect(peerAddressHint({})).toBeNull()
  })
})
