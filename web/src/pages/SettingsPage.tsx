import { useEffect, useMemo, useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  Sun,
  MoonStars,
  Monitor,
  Check,
  Database,
  Eye,
  EyeSlash,
  Fingerprint,
  Key,
  PencilSimple,
  Plus,
  Trash,
  Copy,
  PlugsConnected,
  ArrowClockwise,
  BookOpen,
  UserCircle,
  Warning,
} from '@phosphor-icons/react'
import { PageHeader } from '@/components/PageHeader'
import { Card, CardBody, CardFooter, CardHeader } from '@/components/ui/Card'
import { Switch } from '@/components/ui/Switch'
import { Button } from '@/components/ui/Button'
import { Input } from '@/components/ui/Input'
import { Badge } from '@/components/ui/Badge'
import { Dialog } from '@/components/ui/Dialog'
import { Table, type Column } from '@/components/ui/Table'
import { ConfirmDialog } from '@/components/ui/ConfirmDialog'
import { EmptyState } from '@/components/ui/EmptyState'
import { SkeletonRows } from '@/components/ui/Skeleton'
import { PillMultiSelect } from '@/components/ui/MultiSelect'
import { useToast } from '@/components/ui/Toast'
import { ErrorState } from '@/components/ErrorState'
import {
  SECRET_MASK,
  defaultEsConfig,
  settingsApi,
  settingsKeys,
  stripMaskedSecrets,
  type EsConfigDraft,
} from '@/api/settings'
import { KEY_PERMISSIONS, keyKeys, keysApi } from '@/api/keys'
import { passkeyKeys, passkeysApi } from '@/api/passkeys'
import { authApi } from '@/api/auth'
import { errorMessage } from '@/api/errors'
import { isPasskeyCancellation, passkeysSupported } from '@/lib/webauthn'
import { useAuthStore } from '@/stores/authStore'
import { useCanWrite } from '@/hooks'
import { useThemeStore, type ThemeMode } from '@/stores/themeStore'
import { supportedLanguages } from '@/i18n'
import { cn } from '@/lib/utils'
import { formatDateTime, formatRelative, fromLocalInputValue, toLocalInputValue } from '@/lib/format'
import type { ApiKey, CreateApiKeyRequest, EsTestResult, PasskeySummary } from '@/api/types'

const langLabels: Record<string, string> = { en: 'English', zh: '中文', ja: '日本語' }

/** Mirrors the server-side minimum in `validate_password`. */
const MIN_PASSWORD = 8

/** Public REST reference shipped with the repository. */
const API_DOCS_URL = 'https://github.com/shuaiZend/PingWAF/blob/main/docs/api.md'

export function SettingsPage() {
  const { t, i18n } = useTranslation()
  const { mode, setMode } = useThemeStore()
  const canWrite = useCanWrite()
  const isAdmin = useAuthStore((s) => s.user?.role) === 'admin'

  const themeOptions: { value: ThemeMode; label: string; icon: typeof Sun }[] = [
    { value: 'light', label: t('theme.light'), icon: Sun },
    { value: 'dark', label: t('theme.dark'), icon: MoonStars },
    { value: 'system', label: t('theme.system'), icon: Monitor },
  ]

  return (
    <div className="animate-slide-up">
      <PageHeader title={t('pages.settings.title')} description={t('pages.settings.description')} />

      <div className="flex max-w-4xl flex-col gap-4">
        <AppearanceCard themeOptions={themeOptions} mode={mode} setMode={setMode} i18n={i18n} />
        <AccountCard />
        <PasskeysCard />
        {isAdmin ? (
          <ElasticsearchCard canWrite={canWrite} />
        ) : (
          <Card>
            <CardHeader title={t('pages.settings.elasticsearch')} />
            <CardBody>
              <p className="flex items-start gap-2 text-sm text-fg-subtle">
                <Warning weight="duotone" className="mt-0.5 h-4 w-4 shrink-0 text-fg-warning" />
                {t('pages.settings.adminOnly')}
              </p>
            </CardBody>
          </Card>
        )}
        <ApiKeysCard canWrite={canWrite} />
      </div>
    </div>
  )
}

/* ── Appearance ─────────────────────────────────────────────────────── */

function AppearanceCard({
  themeOptions,
  mode,
  setMode,
  i18n,
}: {
  themeOptions: { value: ThemeMode; label: string; icon: typeof Sun }[]
  mode: ThemeMode
  setMode: (m: ThemeMode) => void
  i18n: { language?: string; changeLanguage: (lng: string) => Promise<unknown> }
}) {
  const { t } = useTranslation()
  return (
    <Card>
      <CardHeader
        title={t('pages.settings.appearance')}
        description={t('pages.settings.appearanceDescription')}
      />
      <CardBody className="flex flex-col gap-6">
        <div>
          <p className="mb-2 text-[13px] font-medium text-fg-subtle">
            {t('pages.settings.themePreference')}
          </p>
          <div className="grid grid-cols-3 gap-2 sm:max-w-md">
            {themeOptions.map((opt) => {
              const Icon = opt.icon
              const active = mode === opt.value
              return (
                <button
                  key={opt.value}
                  type="button"
                  onClick={() => setMode(opt.value)}
                  className={cn(
                    'flex flex-col items-center gap-2 rounded-lg border p-4 text-sm transition-all',
                    active
                      ? 'border-brand bg-brand-soft text-brand'
                      : 'border-line text-fg-subtle hover:border-fill hover:text-fg',
                  )}
                >
                  <Icon weight="duotone" className="h-6 w-6" />
                  {opt.label}
                </button>
              )
            })}
          </div>
        </div>

        <div>
          <p className="mb-2 text-[13px] font-medium text-fg-subtle">
            {t('pages.settings.languagePreference')}
          </p>
          <div className="flex flex-wrap gap-2">
            {supportedLanguages.map((lng) => {
              const active = i18n.language?.startsWith(lng)
              return (
                <button
                  key={lng}
                  type="button"
                  onClick={() => void i18n.changeLanguage(lng)}
                  className={cn(
                    'inline-flex items-center gap-2 rounded-md border px-4 py-2 text-sm transition-colors',
                    active
                      ? 'border-brand bg-brand-soft text-brand'
                      : 'border-line text-fg-subtle hover:border-fill hover:text-fg',
                  )}
                >
                  {active && <Check weight="bold" className="h-4 w-4" />}
                  {langLabels[lng]}
                </button>
              )
            })}
          </div>
        </div>
      </CardBody>
    </Card>
  )
}

/* ── Account ────────────────────────────────────────────────────────── */

function AccountCard() {
  const { t } = useTranslation()
  const toast = useToast()
  const queryClient = useQueryClient()
  const user = useAuthStore((s) => s.user)
  const setUser = useAuthStore((s) => s.setUser)

  const [name, setName] = useState('')
  const [currentPassword, setCurrentPassword] = useState('')
  const [newPassword, setNewPassword] = useState('')
  const [confirmPassword, setConfirmPassword] = useState('')
  const [showPassword, setShowPassword] = useState(false)
  const [passwordError, setPasswordError] = useState<string | null>(null)

  useEffect(() => {
    setName(user?.name ?? '')
  }, [user?.name])

  const profile = useMutation({
    mutationFn: () => authApi.updateProfile({ name: name.trim() || null }),
    onSuccess: (updated) => {
      setUser(updated)
      void queryClient.invalidateQueries({ queryKey: ['auth'] })
      toast.success(t('pages.settings.profileSaved'))
    },
  })

  const password = useMutation({
    mutationFn: () =>
      authApi.changePassword({
        current_password: currentPassword,
        new_password: newPassword,
      }),
    // A wrong current password is a 400 whose message belongs next to the
    // fields, not in a toast.
    meta: { silentToast: true },
    onSuccess: () => {
      setCurrentPassword('')
      setNewPassword('')
      setConfirmPassword('')
      setPasswordError(null)
      toast.success(t('pages.settings.passwordChanged'))
    },
    onError: (err) => {
      const message = errorMessage(err)
      setPasswordError(
        /current password is incorrect/i.test(message)
          ? t('pages.settings.passwordWrongCurrent')
          : message,
      )
    },
  })

  const submitPassword = (event: FormEvent) => {
    event.preventDefault()
    if (password.isPending) return
    setPasswordError(null)
    if (newPassword.length < MIN_PASSWORD) {
      setPasswordError(t('auth.passwordTooShort', { min: MIN_PASSWORD }))
      return
    }
    if (newPassword === currentPassword) {
      setPasswordError(t('pages.settings.passwordUnchanged'))
      return
    }
    if (newPassword !== confirmPassword) {
      setPasswordError(t('pages.settings.passwordMismatch'))
      return
    }
    password.mutate()
  }

  return (
    <Card>
      <CardHeader
        title={t('pages.settings.account')}
        description={user?.email}
        action={
          <Badge tone={user?.role === 'admin' ? 'brand' : 'neutral'}>
            {t(`user.role.${user?.role ?? 'viewer'}`, user?.role ?? '')}
          </Badge>
        }
      />
      <CardBody className="flex flex-col gap-5">
        <div className="flex flex-col gap-3 sm:flex-row sm:items-end">
          <div className="min-w-0 flex-1">
            <Input
              label={t('pages.settings.displayName')}
              value={name}
              placeholder={user?.email ?? ''}
              prefixIcon={<UserCircle weight="duotone" />}
              onChange={(e) => setName(e.target.value)}
            />
          </div>
          <Button
            variant="secondary"
            loading={profile.isPending}
            disabled={name.trim() === (user?.name ?? '')}
            onClick={() => profile.mutate()}
          >
            {t('common.save')}
          </Button>
        </div>

        <div className="border-t border-line pt-4">
          <p className="mb-3 text-[13px] font-medium text-fg">
            {t('pages.settings.changePassword')}
          </p>
          <form onSubmit={submitPassword} className="flex flex-col gap-4">
            <div className="grid max-w-3xl grid-cols-1 gap-4 sm:grid-cols-3">
              <Input
                type="password"
                label={t('pages.settings.currentPassword')}
                value={currentPassword}
                autoComplete="current-password"
                onChange={(e) => setCurrentPassword(e.target.value)}
              />
              <Input
                label={t('pages.settings.newPassword')}
                type={showPassword ? 'text' : 'password'}
                value={newPassword}
                autoComplete="new-password"
                hint={t('auth.passwordHint', { min: MIN_PASSWORD })}
                suffixIcon={
                  <button
                    type="button"
                    onClick={() => setShowPassword((s) => !s)}
                    aria-label={
                      showPassword ? t('auth.hidePassword') : t('auth.showPassword')
                    }
                    className="transition-colors hover:text-fg"
                  >
                    {showPassword ? (
                      <EyeSlash weight="duotone" />
                    ) : (
                      <Eye weight="duotone" />
                    )}
                  </button>
                }
                onChange={(e) => setNewPassword(e.target.value)}
              />
              <Input
                type={showPassword ? 'text' : 'password'}
                label={t('pages.settings.confirmPassword')}
                value={confirmPassword}
                autoComplete="new-password"
                error={
                  confirmPassword !== '' && confirmPassword !== newPassword
                    ? t('pages.settings.passwordMismatch')
                    : undefined
                }
                onChange={(e) => setConfirmPassword(e.target.value)}
              />
            </div>
            <div className="flex flex-wrap items-center gap-3">
              <Button
                type="submit"
                variant="secondary"
                loading={password.isPending}
                disabled={!currentPassword || !newPassword || !confirmPassword}
              >
                {t('pages.settings.updatePassword')}
              </Button>
              {passwordError && (
                <p role="alert" className="text-[13px] text-fg-danger">
                  {passwordError}
                </p>
              )}
            </div>
          </form>
        </div>
      </CardBody>
    </Card>
  )
}

/* ── Passkeys ───────────────────────────────────────────────────────── */

/**
 * Passkeys bound to the signed-in account.
 *
 * Every account manages its own credentials, so the card is not gated on the
 * admin role — a viewer may sign in with a passkey too.
 */
function PasskeysCard() {
  const { t } = useTranslation()
  const toast = useToast()
  const queryClient = useQueryClient()

  const [addOpen, setAddOpen] = useState(false)
  const [name, setName] = useState('')
  const [addError, setAddError] = useState<string | null>(null)
  const [pendingRename, setPendingRename] = useState<PasskeySummary | null>(null)
  const [renameTo, setRenameTo] = useState('')
  const [pendingDelete, setPendingDelete] = useState<PasskeySummary | null>(null)

  const supported = passkeysSupported()

  const query = useQuery({
    queryKey: passkeyKeys.list(),
    queryFn: () => passkeysApi.list(),
    enabled: supported,
  })
  const passkeys = query.data ?? []

  const invalidate = () => void queryClient.invalidateQueries({ queryKey: passkeyKeys.all })

  const add = useMutation({
    mutationFn: (label: string) => passkeysApi.register(label),
    // Dismissing the browser prompt is not an API failure worth a toast.
    meta: { silentToast: true },
    onSuccess: () => {
      setAddOpen(false)
      setName('')
      setAddError(null)
      invalidate()
      toast.success(t('pages.settings.passkeyAdded'))
    },
    onError: (err) => {
      if (isPasskeyCancellation(err)) return
      setAddError(errorMessage(err))
    },
  })

  const rename = useMutation({
    mutationFn: (passkey: PasskeySummary) => passkeysApi.rename(passkey.id, renameTo.trim()),
    onSuccess: () => {
      setPendingRename(null)
      invalidate()
      toast.success(t('pages.settings.passkeyRenamed'))
    },
  })

  const remove = useMutation({
    mutationFn: (passkey: PasskeySummary) => passkeysApi.remove(passkey.id),
    onSuccess: () => {
      setPendingDelete(null)
      invalidate()
      toast.success(t('pages.settings.passkeyRemoved'))
    },
  })

  const columns: Column<PasskeySummary>[] = useMemo(
    () => [
      {
        key: 'name',
        header: t('common.name'),
        accessor: (p) => p.name,
        sortable: true,
        cell: (p) => (
          <div className="flex min-w-0 items-center gap-2">
            <Fingerprint weight="duotone" className="h-4 w-4 shrink-0 text-fg-subtle" />
            <span className="truncate text-[13px] font-medium text-fg-strong">{p.name}</span>
          </div>
        ),
      },
      {
        key: 'created_at',
        header: t('pages.settings.passkeyCreated'),
        accessor: (p) => p.created_at,
        sortable: true,
        width: '1%',
        cell: (p) => (
          <span className="text-[13px] text-fg-subtle">{formatDateTime(p.created_at)}</span>
        ),
      },
      {
        key: 'last_used_at',
        header: t('pages.settings.passkeyLastUsed'),
        accessor: (p) => p.last_used_at,
        width: '1%',
        cell: (p) => (
          <span className="text-[13px] text-fg-subtle">{formatRelative(p.last_used_at)}</span>
        ),
      },
      {
        key: 'row-actions',
        header: '',
        align: 'right',
        width: '1%',
        cell: (p) => (
          <div className="flex items-center justify-end gap-1">
            <Button
              size="icon"
              variant="ghost"
              aria-label={t('pages.settings.renamePasskey')}
              onClick={() => {
                setRenameTo(p.name)
                setPendingRename(p)
              }}
              icon={<PencilSimple weight="duotone" className="h-4 w-4" />}
            />
            <Button
              size="icon"
              variant="ghost"
              className="hover:text-fg-danger"
              aria-label={t('pages.settings.deletePasskeyTitle')}
              onClick={() => setPendingDelete(p)}
              icon={<Trash weight="duotone" className="h-4 w-4" />}
            />
          </div>
        ),
      },
    ],
    [t],
  )

  return (
    <Card>
      <CardHeader
        title={t('pages.settings.passkeys')}
        description={t('pages.settings.passkeysDescription')}
        action={
          supported && passkeys.length > 0 ? (
            <Button
              size="sm"
              variant="secondary"
              icon={<Plus weight="bold" className="h-4 w-4" />}
              onClick={() => setAddOpen(true)}
            >
              {t('pages.settings.addPasskey')}
            </Button>
          ) : undefined
        }
      />
      <CardBody className={passkeys.length === 0 ? undefined : 'p-0'}>
        {!supported ? (
          <p className="flex items-start gap-2 text-sm text-fg-subtle">
            <Warning weight="duotone" className="mt-0.5 h-4 w-4 shrink-0 text-fg-warning" />
            {t('pages.settings.passkeyUnsupported')}
          </p>
        ) : query.isError && !query.data ? (
          <ErrorState
            variant="inline"
            error={query.error}
            onRetry={() => query.refetch()}
            retrying={query.isFetching}
          />
        ) : query.isPending ? (
          <SkeletonRows rows={2} columns={3} />
        ) : passkeys.length === 0 ? (
          <EmptyState
            className="py-10"
            icon={<Fingerprint weight="duotone" className="h-8 w-8" />}
            title={t('pages.settings.noPasskeys')}
            description={t('pages.settings.noPasskeysDescription')}
            action={
              <Button
                variant="primary"
                icon={<Plus weight="bold" className="h-4 w-4" />}
                onClick={() => setAddOpen(true)}
              >
                {t('pages.settings.addPasskey')}
              </Button>
            }
          />
        ) : (
          <Table columns={columns} data={passkeys} rowKey={(p) => p.id} dense />
        )}
      </CardBody>

      {supported && (
        <CardFooter className="justify-start text-left text-xs text-fg-subtle">
          {t('pages.settings.passkeysDomainHint')}
        </CardFooter>
      )}

      {/* Bind a new authenticator */}
      <Dialog
        open={addOpen}
        onClose={add.isPending ? () => undefined : () => setAddOpen(false)}
        title={t('pages.settings.addPasskey')}
        description={t('pages.settings.passkeysDescription')}
        footer={
          <>
            <Button
              variant="ghost"
              onClick={() => setAddOpen(false)}
              disabled={add.isPending}
            >
              {t('common.cancel')}
            </Button>
            <Button
              variant="primary"
              loading={add.isPending}
              disabled={!name.trim()}
              onClick={() => add.mutate(name.trim())}
            >
              {t('pages.settings.addPasskey')}
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-3">
          <Input
            label={t('pages.settings.passkeyName')}
            value={name}
            autoFocus
            placeholder={t('pages.settings.passkeyNamePlaceholder')}
            error={addError ?? undefined}
            onChange={(e) => setName(e.target.value)}
          />
        </div>
      </Dialog>

      {/* Rename */}
      <Dialog
        open={pendingRename !== null}
        onClose={() => setPendingRename(null)}
        title={t('pages.settings.renamePasskey')}
        description={t('pages.settings.renamePasskeyDescription')}
        footer={
          <>
            <Button variant="ghost" onClick={() => setPendingRename(null)}>
              {t('common.cancel')}
            </Button>
            <Button
              variant="primary"
              loading={rename.isPending}
              disabled={!renameTo.trim()}
              onClick={() => pendingRename && rename.mutate(pendingRename)}
            >
              {t('common.save')}
            </Button>
          </>
        }
      >
        <Input
          label={t('pages.settings.passkeyName')}
          value={renameTo}
          autoFocus
          onChange={(e) => setRenameTo(e.target.value)}
        />
      </Dialog>

      <ConfirmDialog
        open={pendingDelete !== null}
        onClose={() => setPendingDelete(null)}
        onConfirm={() => pendingDelete && remove.mutate(pendingDelete)}
        title={t('pages.settings.deletePasskeyTitle')}
        description={t('pages.settings.deletePasskeyDescription')}
        confirmLabel={t('common.delete')}
        loading={remove.isPending}
      >
        {pendingDelete && (
          <div className="rounded-md border border-line bg-recessed px-3 py-2">
            <p className="text-[13px] font-medium text-fg-strong">{pendingDelete.name}</p>
            <p className="mt-0.5 text-xs text-fg-subtle">
              {formatDateTime(pendingDelete.created_at)}
            </p>
          </div>
        )}
      </ConfirmDialog>
    </Card>
  )
}

/* ── Elasticsearch ──────────────────────────────────────────────────── */

function ElasticsearchCard({ canWrite }: { canWrite: boolean }) {
  const { t } = useTranslation()
  const toast = useToast()
  const queryClient = useQueryClient()

  const [draft, setDraft] = useState<EsConfigDraft>(defaultEsConfig)
  const [urlsText, setUrlsText] = useState('')
  const [dirty, setDirty] = useState(false)
  const [testResult, setTestResult] = useState<EsTestResult | null>(null)

  const settings = useQuery({
    queryKey: settingsKeys.elasticsearch(),
    queryFn: () => settingsApi.getElasticsearch(),
  })

  // Load the stored config into the form, turning `***` into blank fields so a
  // save can never write the mask back over the real secret.
  useEffect(() => {
    const config = settings.data?.config
    if (!config) return
    const stripped = stripMaskedSecrets(config)
    setDraft(stripped)
    setUrlsText(stripped.urls.join('\n'))
    setDirty(false)
    setTestResult(null)
  }, [settings.data])

  const patch = (next: Partial<EsConfigDraft>) => {
    setDraft((d) => ({ ...d, ...next }))
    setDirty(true)
  }

  /** Turns the textarea into the URL list, dropping blanks. */
  const collect = (): EsConfigDraft => ({
    ...draft,
    urls: urlsText
      .split('\n')
      .map((u) => u.trim())
      .filter(Boolean),
  })

  const save = useMutation({
    mutationFn: () => settingsApi.saveElasticsearch(collect()),
    onSuccess: (view) => {
      void queryClient.invalidateQueries({ queryKey: settingsKeys.elasticsearch() })
      setDirty(false)
      if (view.requires_restart) {
        toast.warning(t('pages.settings.esSaved'), t('pages.settings.esRequiresRestart'))
      } else {
        toast.success(t('pages.settings.esSaved'), t('pages.settings.esApplied'))
      }
    },
  })

  const testDraft = useMutation({
    mutationFn: () => settingsApi.testElasticsearch(collect()),
    onSuccess: (result) => setTestResult(result),
  })

  const testLive = useMutation({
    mutationFn: () => settingsApi.testLive(),
    onSuccess: (result) => setTestResult(result),
  })

  const health = settings.data?.health
  const running = settings.data?.running ?? false

  const num = (key: keyof EsConfigDraft, value: string) => {
    const parsed = Number(value)
    patch({ [key]: Number.isFinite(parsed) ? parsed : 0 } as Partial<EsConfigDraft>)
  }

  return (
    <Card>
      <CardHeader
        title={t('pages.settings.elasticsearch')}
        description={t('pages.settings.elasticsearchDescription')}
        action={
          <div className="flex items-center gap-2">
            <span className="inline-flex items-center gap-1.5">
              <span
                className={cn(
                  'h-2 w-2 rounded-full',
                  running ? 'bg-success animate-pulse' : 'bg-fg-subtle/50',
                )}
              />
              <span className="text-xs text-fg-subtle">
                {running ? t('pages.settings.esRunning') : t('pages.settings.esStopped')}
              </span>
            </span>
            {health && (
              <Badge
                tone={
                  health.status === 'green'
                    ? 'success'
                    : health.status === 'yellow'
                      ? 'warning'
                      : 'danger'
                }
              >
                {health.status}
              </Badge>
            )}
          </div>
        }
      />
      <CardBody className="flex flex-col gap-5">
        {settings.isError && !settings.data ? (
          <ErrorState
            variant="inline"
            error={settings.error}
            onRetry={() => settings.refetch()}
            retrying={settings.isFetching}
          />
        ) : settings.isPending ? (
          <SkeletonRows rows={4} columns={2} />
        ) : (
          <>
            {settings.data?.requires_restart && (
              <p className="flex items-start gap-2 rounded-md border border-warning/40 bg-warning/8 px-3 py-2 text-[13px] text-fg">
                <Warning weight="duotone" className="mt-0.5 h-4 w-4 shrink-0 text-fg-warning" />
                {t('pages.settings.esRequiresRestart')}
              </p>
            )}

            <Switch
              checked={draft.enabled}
              onCheckedChange={(enabled) => patch({ enabled })}
              disabled={!canWrite}
              label={t('pages.settings.esEnabled')}
              description={t('pages.settings.esEnabledHint')}
            />

            <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
              <div className="sm:col-span-2">
                <label
                  htmlFor="es-urls"
                  className="mb-1.5 block text-[13px] font-medium text-fg"
                >
                  {t('pages.settings.esUrls')}
                </label>
                <textarea
                  id="es-urls"
                  rows={3}
                  disabled={!canWrite}
                  value={urlsText}
                  placeholder={'http://localhost:9200'}
                  onChange={(e) => {
                    setUrlsText(e.target.value)
                    setDirty(true)
                  }}
                  className="pw-mono w-full rounded-md border border-line bg-recessed px-3 py-2 text-[13px] text-fg placeholder:text-fg-subtle/60 focus:border-focus focus:outline-none disabled:opacity-60"
                />
                <p className="mt-1 text-xs text-fg-subtle">{t('pages.settings.esUrlsHint')}</p>
              </div>

              <Input
                label={t('pages.settings.esIndexPrefix')}
                value={draft.index_prefix}
                disabled={!canWrite}
                onChange={(e) => patch({ index_prefix: e.target.value })}
              />
              <Input
                label={t('pages.settings.esUsername')}
                value={draft.username ?? ''}
                autoComplete="off"
                disabled={!canWrite}
                onChange={(e) => patch({ username: e.target.value || null })}
              />
              <Input
                type="password"
                label={t('pages.settings.esPassword')}
                value={draft.password ?? ''}
                autoComplete="new-password"
                placeholder={
                  settings.data?.config.password === SECRET_MASK
                    ? t('pages.settings.secretStored')
                    : ''
                }
                hint={t('pages.settings.secretHint')}
                disabled={!canWrite}
                onChange={(e) => patch({ password: e.target.value || null })}
              />
              <Input
                type="password"
                label={t('pages.settings.esApiKey')}
                value={draft.api_key ?? ''}
                autoComplete="new-password"
                placeholder={
                  settings.data?.config.api_key === SECRET_MASK
                    ? t('pages.settings.secretStored')
                    : ''
                }
                hint={t('pages.settings.apiKeyHint')}
                disabled={!canWrite}
                onChange={(e) => patch({ api_key: e.target.value || null })}
              />
            </div>

            <div className="grid grid-cols-2 gap-4 sm:grid-cols-4">
              <Input
                type="number"
                label={t('pages.settings.esBulkMaxSize')}
                value={draft.bulk_max_size}
                min={1}
                disabled={!canWrite}
                onChange={(e) => num('bulk_max_size', e.target.value)}
              />
              <Input
                type="number"
                label={t('pages.settings.esBulkFlushMs')}
                value={draft.bulk_flush_interval_ms}
                min={1}
                disabled={!canWrite}
                onChange={(e) => num('bulk_flush_interval_ms', e.target.value)}
              />
              <Input
                type="number"
                label={t('pages.settings.esMaxBodySize')}
                value={draft.max_body_size}
                min={0}
                hint={t('pages.settings.bytes')}
                disabled={!canWrite}
                onChange={(e) => num('max_body_size', e.target.value)}
              />
              <Input
                type="number"
                label={t('pages.settings.esRequestTimeout')}
                value={draft.request_timeout_secs}
                min={1}
                disabled={!canWrite}
                onChange={(e) => num('request_timeout_secs', e.target.value)}
              />
              <Input
                label={t('pages.settings.esBufferDir')}
                value={draft.buffer_dir ?? ''}
                placeholder={t('pages.settings.esBufferDirPlaceholder')}
                className="pw-mono text-[13px]"
                disabled={!canWrite}
                onChange={(e) => patch({ buffer_dir: e.target.value || null })}
              />
              <Input
                type="number"
                label={t('pages.settings.esBufferMaxMb')}
                value={draft.buffer_max_size_mb}
                min={1}
                disabled={!canWrite}
                onChange={(e) => num('buffer_max_size_mb', e.target.value)}
              />
              <Input
                type="number"
                label={t('pages.settings.esChannelCapacity')}
                value={draft.channel_capacity}
                min={1}
                disabled={!canWrite}
                onChange={(e) => num('channel_capacity', e.target.value)}
              />
            </div>

            {testResult && (
              <div
                className={cn(
                  'rounded-md border px-3 py-2 text-[13px]',
                  testResult.ok
                    ? 'border-success/40 bg-success/8 text-fg'
                    : 'border-danger/40 bg-danger/8 text-fg-danger',
                )}
              >
                <p className="font-medium">
                  {testResult.ok
                    ? t('pages.settings.esTestOk')
                    : t('pages.settings.esTestFailed')}
                </p>
                {testResult.error && <p className="pw-mono mt-1 break-all text-xs">{testResult.error}</p>}
                {testResult.health && (
                  <p className="mt-1 text-xs text-fg-subtle">
                    {t('pages.settings.esCluster')}: {testResult.health.cluster_name ?? '—'} ·{' '}
                    {t('pages.settings.esNodes')}: {testResult.health.number_of_nodes ?? '—'} ·{' '}
                    {t('pages.settings.esShards')}: {testResult.health.active_shards ?? '—'}
                  </p>
                )}
                <p className="mt-1 text-xs text-fg-subtle">
                  {testResult.template_installed
                    ? t('pages.settings.esTemplateInstalled')
                    : t('pages.settings.esTemplateMissing')}
                </p>
              </div>
            )}

            <div className="flex flex-wrap items-center gap-2 border-t border-line pt-4">
              <Button
                variant="primary"
                disabled={!canWrite || !dirty}
                loading={save.isPending}
                icon={<Database weight="duotone" className="h-4 w-4" />}
                onClick={() => save.mutate()}
              >
                {t('common.save')}
              </Button>
              <Button
                variant="secondary"
                disabled={!canWrite}
                loading={testDraft.isPending}
                icon={<PlugsConnected weight="duotone" className="h-4 w-4" />}
                onClick={() => testDraft.mutate()}
              >
                {t('pages.settings.testDraft')}
              </Button>
              <Button
                variant="ghost"
                loading={testLive.isPending}
                icon={<ArrowClockwise weight="duotone" className="h-4 w-4" />}
                onClick={() => testLive.mutate()}
              >
                {t('pages.settings.testLive')}
              </Button>
              {dirty && (
                <span className="text-xs text-fg-warning">{t('pages.settings.unsavedChanges')}</span>
              )}
            </div>
          </>
        )}
      </CardBody>
    </Card>
  )
}

/* ── API keys ───────────────────────────────────────────────────────── */

function ApiKeysCard({ canWrite }: { canWrite: boolean }) {
  const { t } = useTranslation()
  const toast = useToast()
  const queryClient = useQueryClient()

  const [dialogOpen, setDialogOpen] = useState(false)
  const [name, setName] = useState('')
  const [permissions, setPermissions] = useState<string[]>(['agent', 'read'])
  const [expiresAt, setExpiresAt] = useState('')
  const [secret, setSecret] = useState<ApiKey | null>(null)
  const [pendingRevoke, setPendingRevoke] = useState<ApiKey | null>(null)

  const keysQuery = useQuery({
    queryKey: keyKeys.list(),
    queryFn: () => keysApi.list(),
    select: (page) => page.items,
  })

  const keys = keysQuery.data ?? []

  const create = useMutation({
    mutationFn: (data: CreateApiKeyRequest) => keysApi.create(data),
    onSuccess: (created) => {
      setDialogOpen(false)
      setName('')
      setPermissions(['agent', 'read'])
      setExpiresAt('')
      setSecret(created)
      void queryClient.invalidateQueries({ queryKey: keyKeys.all })
    },
  })

  const revoke = useMutation({
    mutationFn: (id: string) => keysApi.revoke(id),
    onSuccess: (_data, id) => {
      toast.success(t('pages.settings.keyRevoked'), keys.find((k) => k.id === id)?.name)
      setPendingRevoke(null)
      void queryClient.invalidateQueries({ queryKey: keyKeys.all })
    },
  })

  const columns: Column<ApiKey>[] = useMemo(
    () => [
      {
        key: 'name',
        header: t('common.name'),
        accessor: (k) => k.name,
        sortable: true,
        cell: (k) => (
          <div className="min-w-0">
            <p className="truncate text-[13px] font-medium text-fg-strong">{k.name}</p>
            <p className="pw-mono truncate text-xs text-fg-subtle">{k.key_prefix}…</p>
          </div>
        ),
      },
      {
        key: 'permissions',
        header: t('pages.settings.permissions'),
        width: '1%',
        cell: (k) => (
          <div className="flex flex-wrap gap-1">
            {(k.permissions ?? []).map((p) => (
              <Badge key={p} tone={p === 'write' ? 'warning' : p === 'agent' ? 'brand' : 'neutral'}>
                {p}
              </Badge>
            ))}
          </div>
        ),
      },
      {
        key: 'expires_at',
        header: t('pages.settings.expires'),
        accessor: (k) => k.expires_at ?? '',
        width: '1%',
        cell: (k) =>
          k.expires_at ? (
            <span
              className={cn(
                'text-[13px]',
                new Date(k.expires_at).getTime() < Date.now() ? 'text-fg-danger' : 'text-fg-subtle',
              )}
            >
              {formatDateTime(k.expires_at)}
            </span>
          ) : (
            <span className="text-[13px] text-fg-subtle">{t('pages.settings.neverExpires')}</span>
          ),
      },
      {
        key: 'last_used_at',
        header: t('pages.settings.lastUsed'),
        accessor: (k) => k.last_used_at ?? '',
        width: '1%',
        cell: (k) => (
          <span className="text-[13px] text-fg-subtle">
            {k.last_used_at ? formatRelative(k.last_used_at) : t('pages.settings.neverUsed')}
          </span>
        ),
      },
      {
        key: 'created_at',
        header: t('pages.settings.created'),
        accessor: (k) => k.created_at,
        sortable: true,
        width: '1%',
        cell: (k) => (
          <span className="text-[13px] text-fg-subtle">{formatDateTime(k.created_at)}</span>
        ),
      },
      {
        key: 'row-actions',
        header: '',
        align: 'right',
        width: '1%',
        cell: (k) => (
          <Button
            size="icon"
            variant="ghost"
            className="hover:text-fg-danger"
            aria-label={t('pages.settings.revoke')}
            disabled={!canWrite}
            onClick={() => setPendingRevoke(k)}
            icon={<Trash weight="duotone" className="h-4 w-4" />}
          />
        ),
      },
    ],
    [t, canWrite],
  )

  const copySecret = async () => {
    if (!secret?.key) return
    try {
      await navigator.clipboard.writeText(secret.key)
      toast.success(t('pages.settings.copied'))
    } catch {
      toast.error(t('pages.settings.copyFailed'))
    }
  }

  return (
    <Card>
      <CardHeader
        title={t('pages.settings.apiKeys')}
        description={t('pages.settings.apiKeysDescription')}
        action={
          <div className="flex items-center gap-2">
            <a
              href={API_DOCS_URL}
              target="_blank"
              rel="noreferrer noopener"
              className={cn(
                'inline-flex h-8 items-center gap-1.5 rounded-md border border-line bg-elevated px-3',
                'text-[13px] font-medium text-fg shadow-sm transition-colors hover:bg-recessed',
              )}
            >
              <BookOpen weight="duotone" className="h-4 w-4" />
              {t('pages.settings.apiDocs')}
            </a>
            {canWrite && (
              <Button
                size="sm"
                variant="primary"
                icon={<Plus weight="bold" className="h-4 w-4" />}
                onClick={() => setDialogOpen(true)}
              >
                {t('common.create')}
              </Button>
            )}
          </div>
        }
      />
      <CardBody className="p-0">
        {keysQuery.isError && !keysQuery.data ? (
          <div className="p-4">
            <ErrorState
              variant="inline"
              error={keysQuery.error}
              onRetry={() => keysQuery.refetch()}
              retrying={keysQuery.isFetching}
            />
          </div>
        ) : keysQuery.isPending ? (
          <SkeletonRows rows={3} columns={5} />
        ) : keys.length === 0 ? (
          <EmptyState
            className="py-10"
            icon={<Key weight="duotone" className="h-8 w-8" />}
            title={t('pages.settings.noKeys')}
            description={t('pages.settings.noKeysDescription')}
            action={
              canWrite ? (
                <Button
                  variant="primary"
                  icon={<Plus weight="bold" className="h-4 w-4" />}
                  onClick={() => setDialogOpen(true)}
                >
                  {t('common.create')}
                </Button>
              ) : undefined
            }
          />
        ) : (
          <Table columns={columns} data={keys} rowKey={(k) => k.id} dense />
        )}
      </CardBody>

      {/* Create key */}
      <Dialog
        open={dialogOpen}
        onClose={create.isPending ? () => undefined : () => setDialogOpen(false)}
        title={t('pages.settings.createKey')}
        description={t('pages.settings.createKeyDescription')}
        footer={
          <>
            <Button variant="ghost" onClick={() => setDialogOpen(false)} disabled={create.isPending}>
              {t('common.cancel')}
            </Button>
            <Button
              variant="primary"
              loading={create.isPending}
              disabled={!name.trim() || permissions.length === 0}
              onClick={() =>
                create.mutate({
                  name: name.trim(),
                  permissions,
                  expires_at: expiresAt ? fromLocalInputValue(expiresAt) ?? null : null,
                })
              }
            >
              {t('common.create')}
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-4">
          <Input
            label={t('common.name')}
            value={name}
            autoFocus
            placeholder={t('pages.settings.keyNamePlaceholder')}
            onChange={(e) => setName(e.target.value)}
            required
          />
          <PillMultiSelect
            label={t('pages.settings.permissions')}
            hint={t('pages.settings.permissionsHint')}
            min={1}
            value={permissions}
            onChange={setPermissions}
            options={KEY_PERMISSIONS.map((p) => ({
              value: p,
              label: t(`permissions.${p}`, p),
              hint: t(`permissions.${p}Hint`, ''),
            }))}
          />
          <Input
            type="datetime-local"
            label={t('pages.settings.expires')}
            value={expiresAt}
            hint={t('pages.settings.expiresHint')}
            min={toLocalInputValue(new Date())}
            onChange={(e) => setExpiresAt(e.target.value)}
          />
        </div>
      </Dialog>

      {/* One-time secret */}
      <Dialog
        open={secret !== null}
        onClose={() => setSecret(null)}
        title={t('pages.settings.keyCreated')}
        description={t('pages.settings.keyCreatedDescription')}
        footer={
          <>
            <Button variant="secondary" onClick={copySecret} icon={<Copy weight="duotone" className="h-4 w-4" />}>
              {t('pages.settings.copy')}
            </Button>
            <Button variant="primary" onClick={() => setSecret(null)}>
              {t('common.close')}
            </Button>
          </>
        }
      >
        {secret && (
          <div className="flex flex-col gap-3">
            <p className="text-[13px] text-fg-subtle">
              {t('pages.settings.keyName')}: <strong className="text-fg">{secret.name}</strong>
            </p>
            <p className="pw-mono break-all rounded-md border border-brand/40 bg-brand-soft px-3 py-2.5 text-[13px] font-medium text-brand">
              {secret.key}
            </p>
            <p className="flex items-start gap-2 text-xs text-fg-warning">
              <Warning weight="duotone" className="mt-0.5 h-3.5 w-3.5 shrink-0" />
              {t('pages.settings.keyOnlyShownOnce')}
            </p>
          </div>
        )}
      </Dialog>

      <ConfirmDialog
        open={pendingRevoke !== null}
        onClose={() => setPendingRevoke(null)}
        onConfirm={() => pendingRevoke && revoke.mutate(pendingRevoke.id)}
        title={t('pages.settings.revokeTitle')}
        description={t('pages.settings.revokeDescription')}
        confirmLabel={t('pages.settings.revoke')}
        loading={revoke.isPending}
      >
        {pendingRevoke && (
          <div className="rounded-md border border-line bg-recessed px-3 py-2">
            <p className="text-[13px] font-medium text-fg-strong">{pendingRevoke.name}</p>
            <p className="pw-mono mt-0.5 text-xs text-fg-subtle">{pendingRevoke.key_prefix}…</p>
          </div>
        )}
      </ConfirmDialog>
    </Card>
  )
}

export default SettingsPage
