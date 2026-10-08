import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Card, CardBody, CardHeader } from '@/components/ui/Card'
import { Button } from '@/components/ui/Button'
import { Textarea } from '@/components/ui/Textarea'
import { Select } from '@/components/ui/Select'
import { Switch } from '@/components/ui/Switch'
import { PillMultiSelect } from '@/components/ui/MultiSelect'
import { useToast } from '@/components/ui/Toast'
import { sitesApi, siteKeys } from '@/api/sites'
import { errorMessage } from '@/api/errors'
import { useCanWrite } from '@/hooks'
import type { IpGroupResponse, Site } from '@/api/types'
import { parseTrustedRanges, TRUSTED_HEADER_OPTIONS } from './types'

const sameIds = (a: string[], b: string[]) =>
  [...a].sort().join(',') === [...b].sort().join(',')

/**
 * Sites behind a CDN/reverse proxy derive the client IP from a trusted
 * forwarded header; IP blocks, CC rules and logs all key on the resolved
 * address instead of the TCP peer. The trust scope is the union of the
 * hand-entered CIDRs and the ranges of the referenced IP groups (e.g. the
 * built-in Cloudflare subscription).
 */
export function ProxyTrustCard({
  site,
  ipGroups,
}: {
  site: Site
  ipGroups: IpGroupResponse[]
}) {
  const { t } = useTranslation()
  const toast = useToast()
  const queryClient = useQueryClient()
  const canWrite = useCanWrite()
  const [enabled, setEnabled] = useState(site.trust_proxy_headers)
  const [header, setHeader] = useState(
    site.trusted_header || 'x-forwarded-for',
  )
  const [lastHop, setLastHop] = useState(site.trust_last_hop)
  const [ranges, setRanges] = useState(
    site.trusted_proxy_ranges.join('\n'),
  )
  const [groupIds, setGroupIds] = useState(
    site.trusted_proxy_group_ids ?? [],
  )
  const [error, setError] = useState<string | null>(null)

  const savedGroupIds = site.trusted_proxy_group_ids ?? []
  const selectedGroups = ipGroups.filter((g) => groupIds.includes(g.id))
  const parsedRanges = parseTrustedRanges(ranges)
  const effectiveCount =
    parsedRanges === null
      ? null
      : new Set([
          ...parsedRanges,
          ...selectedGroups.flatMap((g) => g.ip_ranges),
        ]).size

  const dirty =
    enabled !== site.trust_proxy_headers ||
    header !== (site.trusted_header || 'x-forwarded-for') ||
    lastHop !== site.trust_last_hop ||
    ranges !== site.trusted_proxy_ranges.join('\n') ||
    !sameIds(groupIds, savedGroupIds)

  const save = useMutation({
    mutationFn: (parsedRanges: string[]) =>
      sitesApi.update(site.id, {
        trust_proxy_headers: enabled,
        trusted_header: enabled ? header : '',
        trust_last_hop: lastHop,
        trusted_proxy_ranges: parsedRanges,
        trusted_proxy_group_ids: groupIds,
      }),
    onSuccess: () => {
      toast.success(t('pages.basic.proxyTrust.saved'))
      void queryClient.invalidateQueries({
        queryKey: siteKeys.detail(site.id),
      })
    },
    onError: (e) => setError(errorMessage(e)),
  })

  return (
    <Card>
      <CardHeader
        title={t('pages.basic.proxyTrust.title')}
        description={t('pages.basic.proxyTrust.description')}
      />
      <CardBody className="space-y-4">
        <Switch
          checked={enabled}
          disabled={!canWrite}
          label={t('pages.basic.proxyTrust.enabled')}
          description={t('pages.basic.proxyTrust.enabledHint')}
          onCheckedChange={setEnabled}
        />
        {enabled && (
          <>
            <Select
              label={t('pages.basic.proxyTrust.header')}
              hint={t('pages.basic.proxyTrust.headerHint')}
              value={header}
              options={TRUSTED_HEADER_OPTIONS.map((o) => ({
                value: o.value,
                label: t(o.labelKey),
              }))}
              disabled={!canWrite}
              onChange={(e) => setHeader(e.target.value)}
            />
            <Switch
              checked={lastHop}
              disabled={!canWrite}
              label={t('pages.basic.proxyTrust.lastHop')}
              description={t('pages.basic.proxyTrust.lastHopHint')}
              onCheckedChange={setLastHop}
            />
            <PillMultiSelect
              label={t('pages.basic.proxyTrust.groups')}
              hint={t('pages.basic.proxyTrust.groupsHint')}
              options={ipGroups.map((g) => ({
                value: g.id,
                label: g.name,
                hint: `${g.ip_ranges.length} CIDR`,
              }))}
              value={groupIds}
              onChange={setGroupIds}
              disabled={!canWrite}
            />
            <Textarea
              label={t('pages.basic.proxyTrust.ranges')}
              hint={t('pages.basic.proxyTrust.rangesHint')}
              value={ranges}
              rows={4}
              mono
              disabled={!canWrite}
              placeholder={'173.245.48.0/20\n2400:cb00::/32'}
              onChange={(e) => setRanges(e.target.value)}
            />
            {effectiveCount !== null && (
              <p className="text-xs leading-relaxed text-fg-subtle">
                {effectiveCount > 0
                  ? t('pages.basic.proxyTrust.effectiveRanges', {
                      count: effectiveCount,
                    })
                  : t('pages.basic.proxyTrust.effectiveEmpty')}
              </p>
            )}
          </>
        )}
        {error && (
          <p
            role="alert"
            className="rounded-md border border-danger/40 bg-danger/8 px-3 py-2 text-[13px] text-fg-danger"
          >
            {error}
          </p>
        )}
        {canWrite && (
          <div className="flex justify-end">
            <Button
              variant="primary"
              loading={save.isPending}
              disabled={!dirty}
              onClick={() => {
                if (parsedRanges === null) {
                  setError(t('pages.basic.proxyTrust.rangesInvalid'))
                  return
                }
                setError(null)
                save.mutate(parsedRanges)
              }}
            >
              {t('common.save')}
            </Button>
          </div>
        )}
      </CardBody>
    </Card>
  )
}
