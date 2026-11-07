import { useTranslation } from 'react-i18next'
import { Input } from '@/components/ui/Input'
import { Select } from '@/components/ui/Select'
import { PATH_MATCH_TYPES, type PathMatchType } from '@/api/types'
import { validatePathScope } from '@/lib/pathScope'

/**
 * The shared "path scope" editor used by IP rules, geo restrictions, basic
 * auth and OIDC: an optional path plus a match-type picker. An empty path
 * means every path of the site — the match-type picker then stays disabled
 * because the server stores `prefix` for it.
 */
export function PathScopeFields({
  path,
  matchType,
  onChange,
  disabled,
  hint,
}: {
  path: string
  matchType: PathMatchType | string
  onChange: (scope: { path: string; match_type: PathMatchType | string }) => void
  disabled?: boolean
  /** Extra hint appended to the path field (e.g. the OIDC semantics note). */
  hint?: string
}) {
  const { t } = useTranslation()
  const empty = path.trim() === ''
  const error = validatePathScope(path, matchType)

  const hints = [
    empty ? t('pages.waf.allPaths') : null,
    !empty && matchType === 'regex' ? t('pages.waf.pathRegexHint') : null,
    hint ?? null,
  ].filter((value): value is string => value !== null)

  return (
    <div className="grid grid-cols-1 gap-3 sm:grid-cols-[1fr_160px]">
      <Input
        label={t('pages.waf.pathScope')}
        value={path}
        disabled={disabled}
        placeholder={t('pages.waf.pathScopeHint')}
        hint={hints.length > 0 ? hints.join(' ') : undefined}
        error={error ?? undefined}
        autoComplete="off"
        onChange={(e) => onChange({ path: e.target.value, match_type: matchType })}
      />
      <Select
        label={t('pages.waf.pathMatchType')}
        value={empty ? 'prefix' : matchType}
        disabled={disabled || empty}
        options={PATH_MATCH_TYPES.map((mt) => ({
          value: mt,
          label: t(`pages.pathExceptions.match_${mt}`),
        }))}
        onChange={(e) => onChange({ path, match_type: e.target.value })}
      />
    </div>
  )
}
