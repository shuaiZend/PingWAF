import { useMemo, useState, type FormEvent } from 'react'
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
  ArrowsCounterClockwise,
  BookOpen,
  BracketsCurly,
  Code,
  FileHtml,
  UserCircle,
  Warning,
  Certificate,
  DownloadSimple,
  LockKey,
  ShieldCheck,
  UploadSimple,
} from '@phosphor-icons/react'
import { PageHeader } from '@/components/PageHeader'
import { Card, CardBody, CardFooter, CardHeader } from '@/components/ui/Card'
import { Switch } from '@/components/ui/Switch'
import { Button } from '@/components/ui/Button'
import { Input } from '@/components/ui/Input'
import { Select } from '@/components/ui/Select'
import { Textarea } from '@/components/ui/Textarea'
import { Badge } from '@/components/ui/Badge'
import { Dialog } from '@/components/ui/Dialog'
import { Table, type Column } from '@/components/ui/Table'
import { ConfirmDialog } from '@/components/ui/ConfirmDialog'
import { EmptyState } from '@/components/ui/EmptyState'
import { SkeletonRows, SkeletonCard } from '@/components/ui/Skeleton'
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
import { downloadCertificateFile, tlsApi, tlsKeys } from '@/api/tls'
import { KEY_PERMISSIONS, keyKeys, keysApi } from '@/api/keys'
import {
  errorPagesApi,
  errorPageKeys,
  ERROR_PAGE_MESSAGES,
  ERROR_PAGE_VARIABLES,
  defaultErrorPageTemplate,
  renderErrorPageTemplate,
} from '@/api/errorPages'
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
import {
  ERROR_PAGE_CONTENT_TYPE_LABELS,
  ERROR_PAGE_CONTENT_TYPES,
  ERROR_PAGE_STATUS_CODES,
  type ErrorPage,
  type ErrorPageContentType,
} from '@/api/types'

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
          <ControlPlaneTlsCard canWrite={canWrite} />
        ) : (
          <AdminOnlyCard title={t('pages.settings.controlPlaneTls')} />
        )}
        {isAdmin ? (
          <ElasticsearchCard canWrite={canWrite} />
        ) : (
          <AdminOnlyCard title={t('pages.settings.elasticsearch')} />
        )}
        {isAdmin ? (
          <LogRetentionCard canWrite={canWrite} />
        ) : (
          <AdminOnlyCard title={t('pages.settings.logRetention')} />
        )}
        {isAdmin ? (
          <ErrorPagesCard canWrite={canWrite} />
        ) : (
          <AdminOnlyCard title={t('pages.settings.errorPages')} />
        )}
        <ApiKeysCard canWrite={canWrite} />
      </div>
    </div>
  )
}

/** Placeholder shown in place of an administrator-only card. */
function AdminOnlyCard({ title }: { title: string }) {
  const { t } = useTranslation()
  return (
    <Card>
      <CardHeader title={title} />
      <CardBody>
        <p className="flex items-start gap-2 text-sm text-fg-subtle">
          <Warning weight="duotone" className="mt-0.5 h-4 w-4 shrink-0 text-fg-warning" />
          {t('pages.settings.adminOnly')}
        </p>
      </CardBody>
    </Card>
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

  // `null` until the first seed: a name already present at mount must still
  // reach the form.
  const serverName = user?.name ?? ''
  const [lastServerName, setLastServerName] = useState<string | null>(null)
  if (serverName !== lastServerName) {
    setLastServerName(serverName)
    setName(serverName)
  }

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
  // save can never write the mask back over the real secret. Render-phase
  // reseed on new query data, so the form is right from the first paint.
  const loadedConfig = settings.data?.config
  const [lastLoadedConfig, setLastLoadedConfig] = useState<typeof loadedConfig | null>(null)
  if (loadedConfig && loadedConfig !== lastLoadedConfig) {
    setLastLoadedConfig(loadedConfig)
    const stripped = stripMaskedSecrets(loadedConfig)
    setDraft(stripped)
    setUrlsText(stripped.urls.join('\n'))
    setDirty(false)
    setTestResult(null)
  }

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

/* ── Log retention ──────────────────────────────────────────────────── */

/** Server-side bounds from `api::log_retention::validate_days`. */
const MIN_RETENTION_DAYS = 1
const MAX_RETENTION_DAYS = 3650

function LogRetentionCard({ canWrite }: { canWrite: boolean }) {
  const { t } = useTranslation()
  const toast = useToast()
  const queryClient = useQueryClient()

  const [accessDays, setAccessDays] = useState('')
  const [securityDays, setSecurityDays] = useState('')
  const [dirty, setDirty] = useState(false)

  const settings = useQuery({
    queryKey: settingsKeys.logRetention(),
    queryFn: () => settingsApi.getLogRetention(),
  })

  const serverRow = settings.data
  const [lastServerRow, setLastServerRow] = useState<typeof serverRow | null>(null)
  if (serverRow && serverRow !== lastServerRow) {
    setLastServerRow(serverRow)
    setAccessDays(String(serverRow.access_log_retention_days))
    setSecurityDays(String(serverRow.security_event_retention_days))
    setDirty(false)
  }

  const parsed = {
    access: Number(accessDays),
    security: Number(securityDays),
  }
  const inRange = (value: number) =>
    Number.isInteger(value) && value >= MIN_RETENTION_DAYS && value <= MAX_RETENTION_DAYS
  const valid = inRange(parsed.access) && inRange(parsed.security)

  const save = useMutation({
    mutationFn: () =>
      settingsApi.saveLogRetention({
        access_log_retention_days: parsed.access,
        security_event_retention_days: parsed.security,
      }),
    onSuccess: (row) => {
      void queryClient.invalidateQueries({ queryKey: settingsKeys.logRetention() })
      setAccessDays(String(row.access_log_retention_days))
      setSecurityDays(String(row.security_event_retention_days))
      setDirty(false)
      toast.success(t('pages.settings.logRetentionSaved'))
    },
  })

  return (
    <Card>
      <CardHeader
        title={t('pages.settings.logRetention')}
        description={t('pages.settings.logRetentionDescription')}
        action={
          <Button
            variant="primary"
            size="sm"
            disabled={!canWrite || !dirty || !valid}
            loading={save.isPending}
            icon={<Trash weight="duotone" className="h-4 w-4" />}
            onClick={() => save.mutate()}
          >
            {t('common.save')}
          </Button>
        }
      />
      <CardBody>
        <div className="grid gap-4 sm:grid-cols-2">
          <Input
            type="number"
            label={t('pages.settings.logRetentionAccess')}
            value={accessDays}
            min={MIN_RETENTION_DAYS}
            max={MAX_RETENTION_DAYS}
            disabled={!canWrite}
            onChange={(e) => {
              setAccessDays(e.target.value)
              setDirty(true)
            }}
          />
          <Input
            type="number"
            label={t('pages.settings.logRetentionSecurity')}
            value={securityDays}
            min={MIN_RETENTION_DAYS}
            max={MAX_RETENTION_DAYS}
            disabled={!canWrite}
            onChange={(e) => {
              setSecurityDays(e.target.value)
              setDirty(true)
            }}
          />
        </div>

        {!valid && (
          <p className="mt-3 flex items-center gap-2 text-xs text-fg-danger">
            <Warning weight="duotone" className="h-4 w-4 shrink-0" />
            {t('pages.settings.logRetentionRange')}
          </p>
        )}
        <p className="mt-3 text-xs text-fg-subtle">{t('pages.settings.logRetentionHint')}</p>
        <p className="mt-1 text-xs text-fg-subtle">{t('pages.settings.logRetentionEsNote')}</p>
      </CardBody>
    </Card>
  )
}

/* ── Custom error pages ─────────────────────────────────────────────── */

interface ErrorPageEditor {
  status_code: number
  name: string
  content_type: ErrorPageContentType
  body_template: string
  enabled: boolean
  existingId: string | null
}

/**
 * The global error page templates, edited from here because they apply to
 * every site. One card per status code; a card opens the template editor for
 * that code (or creates it), and the dialog previews the rendered output with
 * sample variables.
 */
function ErrorPagesCard({ canWrite }: { canWrite: boolean }) {
  const { t } = useTranslation()
  const toast = useToast()
  const queryClient = useQueryClient()

  const [editor, setEditor] = useState<ErrorPageEditor | null>(null)
  const [pendingReset, setPendingReset] = useState<ErrorPageEditor | null>(null)
  const [pendingDelete, setPendingDelete] = useState<ErrorPage | null>(null)

  const pagesQuery = useQuery({
    queryKey: errorPageKeys.list(),
    queryFn: () => errorPagesApi.list(),
    select: (res) => res.items,
  })

  const byStatus = useMemo(() => {
    const map = new Map<number, ErrorPage>()
    for (const p of pagesQuery.data ?? []) map.set(p.status_code, p)
    return map
  }, [pagesQuery.data])

  const invalidate = () => {
    void queryClient.invalidateQueries({ queryKey: errorPageKeys.all })
  }

  const save = useMutation({
    mutationFn: (state: ErrorPageEditor) =>
      state.existingId
        ? errorPagesApi.update(state.existingId, {
            status_code: state.status_code,
            name: state.name,
            content_type: state.content_type,
            body_template: state.body_template,
            enabled: state.enabled,
          })
        : errorPagesApi.upsert({
            status_code: state.status_code,
            name: state.name,
            content_type: state.content_type,
            body_template: state.body_template,
            enabled: state.enabled,
          }),
    onSuccess: () => {
      toast.success(t('pages.errorPages.saved'))
      setEditor(null)
      invalidate()
    },
  })

  const toggle = useMutation({
    mutationFn: ({ page, enabled }: { page: ErrorPage; enabled: boolean }) =>
      errorPagesApi.update(page.id, { enabled }),
    onSuccess: () => invalidate(),
  })

  const remove = useMutation({
    mutationFn: (id: string) => errorPagesApi.delete(id),
    onSuccess: () => {
      toast.success(t('pages.errorPages.deleted'))
      setPendingDelete(null)
      setEditor(null)
      invalidate()
    },
  })

  const openEditor = (statusCode: number) => {
    const existing = byStatus.get(statusCode)
    setEditor({
      status_code: statusCode,
      name: existing?.name ?? ERROR_PAGE_MESSAGES[statusCode] ?? String(statusCode),
      content_type: (existing?.content_type as ErrorPageContentType) ?? 'text/html',
      body_template:
        existing?.body_template ??
        defaultErrorPageTemplate(
          statusCode,
          (existing?.content_type as ErrorPageContentType) ?? 'text/html',
        ),
      enabled: existing?.enabled ?? true,
      existingId: existing?.id ?? null,
    })
  }

  const previewHtml = editor ? renderErrorPageTemplate(editor.body_template) : ''

  return (
    <>
      <Card>
        <CardHeader
          title={t('pages.errorPages.title')}
          description={t('pages.errorPages.description')}
          action={
            <Button
              variant="secondary"
              size="sm"
              loading={pagesQuery.isFetching}
              onClick={() => pagesQuery.refetch()}
              icon={<ArrowClockwise weight="duotone" className="h-4 w-4" />}
            >
              {t('common.refresh')}
            </Button>
          }
        />
        <CardBody>
          {pagesQuery.isError && !pagesQuery.data ? (
            <ErrorState
              error={pagesQuery.error}
              onRetry={() => pagesQuery.refetch()}
              retrying={pagesQuery.isFetching}
            />
          ) : pagesQuery.isPending ? (
            <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
              {ERROR_PAGE_STATUS_CODES.map((c) => (
                <SkeletonCard key={c} />
              ))}
            </div>
          ) : (
            <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
              {ERROR_PAGE_STATUS_CODES.map((code) => {
                const page = byStatus.get(code)
                const customized = Boolean(page)
                return (
                  <Card
                    key={code}
                    interactive
                    padded
                    onClick={() => canWrite && openEditor(code)}
                    className={cn('cursor-pointer', !canWrite && 'cursor-default')}
                  >
                    <div className="flex items-start justify-between gap-3">
                      <div className="flex items-center gap-3">
                        <span
                          className={cn(
                            'flex h-12 w-12 shrink-0 items-center justify-center rounded-lg text-lg font-bold tabular-nums',
                            page?.enabled
                              ? 'bg-brand-soft text-brand'
                              : 'bg-recessed text-fg-subtle',
                          )}
                        >
                          {code}
                        </span>
                        <div className="min-w-0">
                          <p className="truncate text-sm font-medium text-fg-strong">
                            {page?.name || ERROR_PAGE_MESSAGES[code]}
                          </p>
                          <p className="mt-0.5 text-xs uppercase tracking-wide text-fg-subtle">
                            {page?.content_type ?? 'text/html'}
                          </p>
                        </div>
                      </div>
                      {customized ? (
                        <Badge tone="brand" size="sm">
                          {t('pages.errorPages.customized')}
                        </Badge>
                      ) : (
                        <Badge tone="neutral" size="sm">
                          {t('pages.errorPages.default')}
                        </Badge>
                      )}
                    </div>
                    <div className="mt-4 flex items-center justify-between">
                      <span className="text-xs text-fg-subtle">
                        {customized
                          ? t('pages.errorPages.clickToEdit')
                          : t('pages.errorPages.clickToCreate')}
                      </span>
                      {page && (
                        <span onClick={(e) => e.stopPropagation()}>
                          <Switch
                            size="sm"
                            checked={page.enabled}
                            disabled={!canWrite || toggle.isPending}
                            aria-label={`${t('common.enabled')}: ${code}`}
                            onCheckedChange={(enabled) => toggle.mutate({ page, enabled })}
                          />
                        </span>
                      )}
                    </div>
                  </Card>
                )
              })}
            </div>
          )}
        </CardBody>
      </Card>

      <Dialog
        open={editor !== null}
        onClose={save.isPending ? () => undefined : () => setEditor(null)}
        size="lg"
        title={editor ? `${editor.status_code} · ${t('pages.errorPages.editTitle')}` : ''}
        description={t('pages.errorPages.editDescription')}
        className="max-w-4xl"
        footer={
          editor && (
            <>
              {editor.existingId && canWrite && (
                <Button
                  variant="ghost"
                  className="mr-auto hover:text-fg-danger"
                  onClick={() => {
                    const page = byStatus.get(editor.status_code)
                    if (page) setPendingDelete(page)
                  }}
                  icon={<FileHtml weight="duotone" className="h-4 w-4" />}
                >
                  {t('pages.errorPages.delete')}
                </Button>
              )}
              <Button
                variant="secondary"
                onClick={() => setPendingReset(editor)}
                icon={<ArrowsCounterClockwise weight="duotone" className="h-4 w-4" />}
              >
                {t('pages.errorPages.resetDefault')}
              </Button>
              <Button variant="ghost" onClick={() => setEditor(null)} disabled={save.isPending}>
                {t('common.cancel')}
              </Button>
              <Button
                variant="primary"
                onClick={() => editor && save.mutate(editor)}
                loading={save.isPending}
              >
                {t('common.save')}
              </Button>
            </>
          )
        }
      >
        {editor && (
          <div className="flex flex-col gap-4">
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
              <Input
                label={t('pages.errorPages.pageName')}
                value={editor.name}
                onChange={(e) => setEditor({ ...editor, name: e.target.value })}
              />
              <Select
                label={t('pages.errorPages.contentType')}
                value={editor.content_type}
                options={ERROR_PAGE_CONTENT_TYPES.map((c) => ({
                  value: c,
                  label: ERROR_PAGE_CONTENT_TYPE_LABELS[c] ?? c.toUpperCase(),
                }))}
                onChange={(e) => {
                  const content_type = e.target.value as ErrorPageContentType
                  setEditor({ ...editor, content_type })
                }}
              />
              <div className="flex items-end pb-1">
                <Switch
                  checked={editor.enabled}
                  onCheckedChange={(enabled) => setEditor({ ...editor, enabled })}
                  label={t('common.enabled')}
                />
              </div>
            </div>

            <div className="grid grid-cols-1 gap-4 lg:grid-cols-[1fr_1fr_220px]">
              {/* Template */}
              <div className="flex flex-col gap-1.5">
                <span className="flex items-center gap-1.5 text-[13px] font-medium text-fg">
                  <Code weight="duotone" className="h-4 w-4" />
                  {t('pages.errorPages.template')}
                </span>
                <Textarea
                  mono
                  rows={18}
                  value={editor.body_template}
                  onChange={(e) => setEditor({ ...editor, body_template: e.target.value })}
                />
              </div>

              {/* Preview */}
              <div className="flex flex-col gap-1.5">
                <span className="flex items-center gap-1.5 text-[13px] font-medium text-fg">
                  <Eye weight="duotone" className="h-4 w-4" />
                  {t('pages.errorPages.preview')}
                </span>
                <div className="h-[388px] overflow-hidden rounded-md border border-line bg-white">
                  {editor.content_type === 'text/html' ? (
                    <iframe
                      title={t('pages.errorPages.preview')}
                      className="h-full w-full"
                      sandbox=""
                      srcDoc={previewHtml}
                    />
                  ) : (
                    <pre className="pw-mono h-full w-full overflow-auto bg-recessed p-3 text-xs text-fg">
                      {previewHtml}
                    </pre>
                  )}
                </div>
              </div>

              {/* Variables */}
              <div className="flex flex-col gap-1.5">
                <span className="flex items-center gap-1.5 text-[13px] font-medium text-fg">
                  <BracketsCurly weight="duotone" className="h-4 w-4" />
                  {t('pages.errorPages.variables')}
                </span>
                <div className="flex max-h-[388px] flex-col gap-1 overflow-y-auto rounded-md border border-line bg-recessed/40 p-2">
                  {ERROR_PAGE_VARIABLES.map((v) => (
                    <button
                      key={v.name}
                      type="button"
                      title={v.description}
                      onClick={() =>
                        setEditor((e) =>
                          e ? { ...e, body_template: `${e.body_template}{{${v.name}}}` } : e,
                        )
                      }
                      className="group flex flex-col items-start rounded px-1.5 py-1 text-left transition-colors hover:bg-elevated"
                    >
                      <span className="pw-mono text-xs text-brand">{`{{${v.name}}}`}</span>
                      <span className="text-[11px] leading-tight text-fg-subtle">
                        {v.description}
                      </span>
                    </button>
                  ))}
                </div>
              </div>
            </div>
          </div>
        )}
      </Dialog>

      <ConfirmDialog
        open={pendingReset !== null}
        onClose={() => setPendingReset(null)}
        tone="primary"
        onConfirm={() => {
          if (!pendingReset) return
          setEditor({
            ...pendingReset,
            content_type: pendingReset.content_type,
            body_template: defaultErrorPageTemplate(
              pendingReset.status_code,
              pendingReset.content_type,
            ),
          })
          setPendingReset(null)
        }}
        title={t('pages.errorPages.resetTitle')}
        description={t('pages.errorPages.resetDescription')}
        confirmLabel={t('pages.errorPages.resetDefault')}
      />

      <ConfirmDialog
        open={pendingDelete !== null}
        onClose={() => setPendingDelete(null)}
        onConfirm={() => pendingDelete && remove.mutate(pendingDelete.id)}
        title={t('pages.errorPages.deleteTitle')}
        description={t('pages.errorPages.deleteDescription')}
        confirmLabel={t('common.delete')}
        loading={remove.isPending}
      />
    </>
  )
}

/* ── Control-plane HTTPS ────────────────────────────────────────────── */

/**
 * Whether a certificate's SANs cover the host the console is open on.
 *
 * A wildcard covers exactly one label (`*.example.com` matches
 * `waf.example.com` but not `a.b.example.com`), mirroring how browsers match.
 * IP addresses only match a literal SAN, so a bare-IP deployment needs an IP
 * entry — which is why the self-signed generator adds one.
 */
function sanCoversHost(sans: string[], host: string): boolean {
  const target = host.trim().toLowerCase()
  if (!target) return true
  return sans.some((san) => {
    const value = san.trim().toLowerCase()
    if (!value) return false
    if (value === target) return true
    if (!value.startsWith('*.')) return false
    const suffix = value.slice(2)
    const dot = target.indexOf('.')
    return dot > 0 && target.slice(dot + 1) === suffix
  })
}

function TlsDetail({
  label,
  value,
  mono = false,
  className,
}: {
  label: string
  value: string
  mono?: boolean
  className?: string
}) {
  return (
    <div className={className}>
      <dt className="text-xs text-fg-subtle">{label}</dt>
      <dd className={cn('mt-0.5 break-all text-fg', mono && 'pw-mono text-xs')}>{value}</dd>
    </div>
  )
}

/**
 * The certificate the dashboard itself is served with.
 *
 * Passkeys (and the browser clipboard) require a secure origin, so this card is
 * the only place an operator can look at — and replace — the certificate
 * without touching the server's files. An upload takes effect immediately: the
 * running listener swaps the pair through its reloadable resolver.
 */
function ControlPlaneTlsCard({ canWrite }: { canWrite: boolean }) {
  const { t } = useTranslation()
  const toast = useToast()
  const queryClient = useQueryClient()

  const [uploadOpen, setUploadOpen] = useState(false)
  const [regenOpen, setRegenOpen] = useState(false)
  const [certPem, setCertPem] = useState('')
  const [keyPem, setKeyPem] = useState('')

  const status = useQuery({
    queryKey: tlsKeys.status(),
    queryFn: () => tlsApi.getStatus(),
  })

  const view = status.data
  const certificate = view?.certificate ?? null

  const invalidate = () => {
    void queryClient.invalidateQueries({ queryKey: tlsKeys.all })
  }

  const upload = useMutation({
    mutationFn: () =>
      tlsApi.uploadCertificate({
        cert_pem: certPem.trim(),
        key_pem: keyPem.trim(),
      }),
    onSuccess: () => {
      invalidate()
      setUploadOpen(false)
      setCertPem('')
      setKeyPem('')
      toast.success(t('pages.settings.tlsUploaded'))
    },
  })

  const regenerate = useMutation({
    mutationFn: () => tlsApi.generateSelfSigned({}),
    onSuccess: () => {
      invalidate()
      setRegenOpen(false)
      toast.success(t('pages.settings.tlsGenerated'))
    },
  })

  const download = useMutation({
    mutationFn: () => tlsApi.downloadCertificate(),
    onSuccess: (pem) => {
      downloadCertificateFile(
        certificate?.source === 'self_signed'
          ? 'pingwaf-control-plane-self-signed.crt'
          : 'pingwaf-control-plane.crt',
        pem,
      )
      toast.success(t('pages.settings.tlsDownloaded'))
    },
  })

  const copyFingerprint = async () => {
    if (!certificate) return
    try {
      await navigator.clipboard.writeText(certificate.fingerprint_sha256)
      toast.success(t('pages.settings.copied'))
    } catch {
      toast.error(t('pages.settings.copyFailed'))
    }
  }

  const closeUpload = () => {
    if (upload.isPending) return
    setUploadOpen(false)
  }

  const selfSigned = certificate?.source === 'self_signed'
  const host = window.location.hostname
  const covered = certificate ? sanCoversHost(certificate.sans, host) : true
  const expiringSoon =
    certificate !== null &&
    certificate.expires_in_days <= 30 &&
    certificate.expires_in_days >= 0

  return (
    <Card>
      <CardHeader
        title={t('pages.settings.controlPlaneTls')}
        description={t('pages.settings.controlPlaneTlsDescription')}
        action={
          view && (
            <Badge tone={view.enabled ? 'success' : 'neutral'} dot>
              {view.enabled
                ? t('pages.settings.tlsEnabled')
                : t('pages.settings.tlsDisabled')}
            </Badge>
          )
        }
      />
      <CardBody className="flex flex-col gap-5">
        {status.isError && !view ? (
          <ErrorState
            variant="inline"
            error={status.error}
            onRetry={() => status.refetch()}
            retrying={status.isFetching}
          />
        ) : !view ? (
          <SkeletonRows rows={3} columns={2} />
        ) : (
          <>
            {!view.enabled && (
              <p className="flex items-start gap-2 rounded-md border border-warning/40 bg-warning/8 px-3 py-2 text-[13px] text-fg">
                <Warning weight="duotone" className="mt-0.5 h-4 w-4 shrink-0 text-fg-warning" />
                {t('pages.settings.tlsDisabledHint')}
              </p>
            )}
            {certificate && !covered && (
              <p className="flex items-start gap-2 rounded-md border border-danger/40 bg-danger/8 px-3 py-2 text-[13px] text-fg">
                <Warning weight="duotone" className="mt-0.5 h-4 w-4 shrink-0 text-fg-danger" />
                {t('pages.settings.tlsHostMismatch', { host })}
              </p>
            )}
            {selfSigned && (
              <p className="flex items-start gap-2 rounded-md border border-warning/40 bg-warning/8 px-3 py-2 text-[13px] text-fg">
                <ShieldCheck weight="duotone" className="mt-0.5 h-4 w-4 shrink-0 text-fg-warning" />
                {t('pages.settings.tlsSelfSignedHint')}
              </p>
            )}
            {expiringSoon && (
              <p className="flex items-start gap-2 rounded-md border border-warning/40 bg-warning/8 px-3 py-2 text-[13px] text-fg">
                <Warning weight="duotone" className="mt-0.5 h-4 w-4 shrink-0 text-fg-warning" />
                {t('pages.settings.tlsExpiringSoon', {
                  count: certificate?.expires_in_days ?? 0,
                })}
              </p>
            )}

            {certificate ? (
              <div className="rounded-md border border-line bg-recessed px-4 py-3">
                <dl className="grid grid-cols-1 gap-x-6 gap-y-3 sm:grid-cols-2">
                  <TlsDetail
                    label={t('pages.settings.tlsSource')}
                    value={
                      selfSigned
                        ? t('pages.settings.tlsSourceSelfSigned')
                        : t('pages.settings.tlsSourceUploaded')
                    }
                    className="sm:col-span-2"
                  />
                  <TlsDetail
                    label={t('pages.settings.tlsSubject')}
                    value={certificate.subject_dn}
                    mono
                    className="sm:col-span-2"
                  />
                  <TlsDetail
                    label={t('pages.settings.tlsValidity')}
                    value={`${formatDateTime(certificate.not_before)} → ${formatDateTime(certificate.not_after)}`}
                    className="sm:col-span-2"
                  />
                  <TlsDetail
                    label={t('pages.settings.tlsExpiresIn')}
                    value={t('pages.settings.tlsDays', {
                      count: certificate.expires_in_days,
                    })}
                  />
                  <TlsDetail
                    label={t('pages.settings.tlsSerial')}
                    value={certificate.serial}
                    mono
                  />
                  <TlsDetail
                    label={t('pages.settings.tlsFingerprint')}
                    value={certificate.fingerprint_sha256}
                    mono
                    className="sm:col-span-2"
                  />
                  <div className="sm:col-span-2">
                    <dt className="text-xs text-fg-subtle">
                      {t('pages.settings.tlsSans')}
                    </dt>
                    <dd className="mt-1 flex flex-wrap gap-1.5">
                      {certificate.sans.length === 0 ? (
                        <span className="text-[13px] text-fg-subtle">—</span>
                      ) : (
                        certificate.sans.map((san) => (
                          <Badge key={san} size="sm" tone="neutral" className="pw-mono">
                            {san}
                          </Badge>
                        ))
                      )}
                    </dd>
                  </div>
                </dl>
              </div>
            ) : (
              <EmptyState
                icon={<LockKey weight="duotone" />}
                title={t('pages.settings.tlsNoCertificate')}
                description={t('pages.settings.tlsNoCertificateDescription')}
                className="py-8"
              />
            )}

            <div className="flex flex-wrap gap-2">
              <Button
                variant="secondary"
                disabled={!certificate}
                loading={download.isPending}
                icon={<DownloadSimple weight="duotone" className="h-4 w-4" />}
                onClick={() => download.mutate()}
              >
                {t('pages.settings.tlsDownload')}
              </Button>
              <Button
                variant="ghost"
                disabled={!certificate}
                icon={<Copy weight="duotone" className="h-4 w-4" />}
                onClick={copyFingerprint}
              >
                {t('pages.settings.tlsCopyFingerprint')}
              </Button>
              <Button
                disabled={!canWrite}
                icon={<UploadSimple weight="duotone" className="h-4 w-4" />}
                onClick={() => setUploadOpen(true)}
              >
                {t('pages.settings.tlsUpload')}
              </Button>
              <Button
                variant="secondary"
                disabled={!canWrite}
                icon={<LockKey weight="duotone" className="h-4 w-4" />}
                onClick={() => setRegenOpen(true)}
              >
                {t('pages.settings.tlsGenerate')}
              </Button>
            </div>
            <p className="flex items-start gap-2 text-xs leading-relaxed text-fg-subtle">
              <Certificate weight="duotone" className="mt-0.5 h-4 w-4 shrink-0" />
              {t('pages.settings.tlsApplyHint')}
            </p>
          </>
        )}
      </CardBody>

      <Dialog
        open={uploadOpen}
        onClose={closeUpload}
        size="md"
        title={t('pages.settings.tlsUploadTitle')}
        description={t('pages.settings.tlsUploadDescription')}
        footer={
          <>
            <Button variant="secondary" onClick={closeUpload} disabled={upload.isPending}>
              {t('common.cancel')}
            </Button>
            <Button
              loading={upload.isPending}
              disabled={!certPem.trim() || !keyPem.trim()}
              onClick={() => upload.mutate()}
            >
              {t('pages.settings.tlsUpload')}
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-4">
          <Textarea
            label={t('pages.settings.tlsCertPem')}
            hint={t('pages.settings.tlsCertPemHint')}
            mono
            rows={7}
            value={certPem}
            placeholder="-----BEGIN CERTIFICATE-----"
            onChange={(e) => setCertPem(e.target.value)}
          />
          <Textarea
            label={t('pages.settings.tlsKeyPem')}
            hint={t('pages.settings.tlsKeyPemHint')}
            mono
            rows={7}
            value={keyPem}
            placeholder="-----BEGIN PRIVATE KEY-----"
            onChange={(e) => setKeyPem(e.target.value)}
          />
          {upload.isError && (
            <p className="text-[13px] font-medium text-fg-danger">
              {errorMessage(upload.error)}
            </p>
          )}
        </div>
      </Dialog>

      <ConfirmDialog
        open={regenOpen}
        onClose={() => setRegenOpen(false)}
        onConfirm={() => regenerate.mutate()}
        tone="primary"
        title={t('pages.settings.tlsGenerateConfirmTitle')}
        description={t('pages.settings.tlsGenerateConfirm')}
        confirmLabel={t('pages.settings.tlsGenerate')}
        loading={regenerate.isPending}
      >
        {(view?.default_sans.length ?? 0) > 0 && (
          <p className="text-xs text-fg-subtle">
            {t('pages.settings.tlsDefaultSans')}:{' '}
            <span className="pw-mono">{view?.default_sans.join(', ')}</span>
          </p>
        )}
      </ConfirmDialog>
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
