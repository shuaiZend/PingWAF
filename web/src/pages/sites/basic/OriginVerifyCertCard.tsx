import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router-dom'
import { Lock } from '@phosphor-icons/react'
import { Card, CardBody, CardHeader } from '@/components/ui/Card'
import { Button } from '@/components/ui/Button'

/**
 * Read-only summary of the site-level origin certificate check. The value is
 * edited on the SSL page (the single source of truth) — this card only shows
 * the current state and links there, reusing the site detail already fetched
 * by the Basic page (no extra request).
 */
export function OriginVerifyCertCard({
  siteId,
  originVerifyCert,
}: {
  siteId: string
  /** `null` means the site follows the global default. */
  originVerifyCert: boolean | null
}) {
  const { t } = useTranslation()
  const navigate = useNavigate()

  return (
    <Card>
      <CardHeader
        title={t('pages.basic.originVerifyCard.title')}
        description={t('pages.basic.originVerifyCard.description')}
        action={
          <Button
            variant="secondary"
            size="sm"
            onClick={() => navigate(`/sites/${siteId}/ssl`)}
          >
            {t('pages.basic.originVerifyCard.editSsl')}
          </Button>
        }
      />
      <CardBody>
        <span
          className={
            'flex items-center gap-2 text-sm font-medium ' +
            (originVerifyCert === null
              ? 'text-fg'
              : originVerifyCert
                ? 'text-success'
                : 'text-warning')
          }
        >
          <Lock weight="duotone" className="h-4 w-4" />
          {originVerifyCert === null
            ? t('pages.basic.originVerifyCard.followGlobal')
            : originVerifyCert
              ? t('pages.basic.originVerifyCard.enabled')
              : t('pages.basic.originVerifyCard.disabled')}
        </span>
        <p className="mt-1 text-xs text-fg-subtle">
          {t('pages.basic.originVerifyCard.note')}
        </p>
      </CardBody>
    </Card>
  )
}
