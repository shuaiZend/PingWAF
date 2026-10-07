import { useMemo, useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  BookOpen,
  Check,
  Copy,
  Eye,
  EyeSlash,
  Key,
  PencilSimple,
  Plus,
  Prohibit,
  Trash,
  UserCircle,
  Warning,
} from '@phosphor-icons/react'
import { PageHeader } from '@/components/PageHeader'
import { Card, CardBody, CardHeader } from '@/components/ui/Card'
import { Button } from '@/components/ui/Button'
import { Input } from '@/components/ui/Input'
import { Select } from '@/components/ui/Select'
import { Badge } from '@/components/ui/Badge'
import { Dialog } from '@/components/ui/Dialog'
import { Table, type Column } from '@/components/ui/Table'
import { ConfirmDialog } from '@/components/ui/ConfirmDialog'
import { EmptyState } from '@/components/ui/EmptyState'
import { SkeletonRows } from '@/components/ui/Skeleton'
import { PillMultiSelect } from '@/components/ui/MultiSelect'
import { useToast } from '@/components/ui/Toast'
import { ErrorState } from '@/components/ErrorState'
import { KEY_PERMISSIONS, keyKeys, keysApi } from '@/api/keys'
import { usersApi, usersKeys } from '@/api/users'
import { authApi } from '@/api/auth'
import { errorMessage } from '@/api/errors'
import { useAuthStore } from '@/stores/authStore'
import { useCanWrite } from '@/hooks'
import { cn } from '@/lib/utils'
import { formatDateTime, formatRelative, fromLocalInputValue, toLocalInputValue } from '@/lib/format'
import type {
  ApiKey,
  CreateApiKeyRequest,
  CreateUserRequest,
  UpdateUserRequest,
  User,
} from '@/api/types'

/** Mirrors the server-side minimum in `validate_password`. */
const MIN_PASSWORD = 8

/** Public REST reference shipped with the repository. */
const API_DOCS_URL = 'https://github.com/shuaiZend/PingWAF/blob/main/docs/api.md'

/** Roles an administrator may assign; mirrors `models::role`. */
const USER_ROLES = ['admin', 'auditor', 'viewer'] as const

export function AccountPage() {
  const { t } = useTranslation()
  const canWrite = useCanWrite()
  const isAdmin = useAuthStore((s) => s.user?.role) === 'admin'

  return (
    <div className="animate-slide-up">
      <PageHeader title={t('pages.account.title')} description={t('pages.account.description')} />
      <div className="flex max-w-4xl flex-col gap-4">
        <AccountCard />
        {isAdmin && <UsersAdminCard />}
        <ApiKeysCard canWrite={canWrite} />
      </div>
    </div>
  )
}

/* ── Profile & password ─────────────────────────────────────────────── */

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
    onSuccess: async () => {
      setCurrentPassword('')
      setNewPassword('')
      setConfirmPassword('')
      setPasswordError(null)
      // Re-sync the stored profile so the `must_change_password` gate clears
      // (and the role badge stays fresh) without a reload.
      try {
        setUser(await authApi.me())
      } catch {
        /* keep the stale profile; the next reload re-syncs */
      }
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

/* ── User management (admin) ────────────────────────────────────────── */

function UsersAdminCard() {
  const { t } = useTranslation()
  const toast = useToast()
  const queryClient = useQueryClient()
  const me = useAuthStore((s) => s.user)
  const setUser = useAuthStore((s) => s.setUser)

  const [createOpen, setCreateOpen] = useState(false)
  const [email, setEmail] = useState('')
  const [name, setName] = useState('')
  const [password, setPassword] = useState('')
  const [role, setRole] = useState<string>('viewer')
  const [editing, setEditing] = useState<User | null>(null)
  const [editName, setEditName] = useState('')
  const [editRole, setEditRole] = useState<string>('viewer')
  const [pendingDisable, setPendingDisable] = useState<User | null>(null)

  const usersQuery = useQuery({
    queryKey: usersKeys.list(),
    queryFn: () => usersApi.list(),
    select: (page) => page.items,
  })

  const users = usersQuery.data ?? []

  const create = useMutation({
    mutationFn: (data: CreateUserRequest) => usersApi.create(data),
    onSuccess: () => {
      setCreateOpen(false)
      setEmail('')
      setName('')
      setPassword('')
      setRole('viewer')
      void queryClient.invalidateQueries({ queryKey: usersKeys.all })
      toast.success(t('pages.account.users.created'))
    },
  })

  const update = useMutation({
    mutationFn: ({ id, data }: { id: string; data: UpdateUserRequest }) =>
      usersApi.update(id, data),
    onSuccess: (updated) => {
      setEditing(null)
      setPendingDisable(null)
      void queryClient.invalidateQueries({ queryKey: usersKeys.all })
      // A self-update (display name) must refresh the stored session profile.
      if (updated.id === me?.id) setUser(updated)
      toast.success(t('pages.account.users.updated'))
    },
  })

  const openEdit = (user: User) => {
    setEditing(user)
    setEditName(user.name ?? '')
    setEditRole(USER_ROLES.includes(user.role as (typeof USER_ROLES)[number]) ? user.role : 'viewer')
  }

  const columns: Column<User>[] = useMemo(
    () => [
      {
        key: 'account',
        header: t('pages.account.users.colAccount'),
        accessor: (u) => u.email,
        cell: (u) => (
          <div className="min-w-0">
            <p className="flex items-center gap-2 truncate text-[13px] font-medium text-fg-strong">
              {u.email}
              {u.id === me?.id && (
                <Badge size="sm" tone="info">
                  {t('pages.account.users.you')}
                </Badge>
              )}
            </p>
            {u.name && <p className="truncate text-xs text-fg-subtle">{u.name}</p>}
          </div>
        ),
      },
      {
        key: 'role',
        header: t('pages.account.users.role'),
        width: '1%',
        cell: (u) => (
          <Badge tone={u.role === 'admin' ? 'brand' : u.role === 'auditor' ? 'info' : 'neutral'}>
            {t(`user.role.${u.role}`, u.role)}
          </Badge>
        ),
      },
      {
        key: 'status',
        header: t('pages.account.users.colStatus'),
        width: '1%',
        cell: (u) =>
          u.disabled ? (
            <Badge tone="danger">{t('pages.account.users.disabled')}</Badge>
          ) : (
            <Badge tone="success">{t('pages.account.users.active')}</Badge>
          ),
      },
      {
        key: 'created_at',
        header: t('pages.settings.created'),
        accessor: (u) => u.created_at,
        sortable: true,
        width: '1%',
        cell: (u) => (
          <span className="text-[13px] text-fg-subtle">{formatDateTime(u.created_at)}</span>
        ),
      },
      {
        key: 'row-actions',
        header: '',
        align: 'right',
        width: '1%',
        cell: (u) => (
          <div className="flex items-center justify-end gap-1">
            <Button
              size="icon"
              variant="ghost"
              aria-label={t('pages.account.users.edit')}
              onClick={() => openEdit(u)}
              icon={<PencilSimple weight="duotone" className="h-4 w-4" />}
            />
            {u.id !== me?.id &&
              (u.disabled ? (
                <Button
                  size="icon"
                  variant="ghost"
                  aria-label={t('pages.account.users.enable')}
                  disabled={update.isPending}
                  onClick={() => update.mutate({ id: u.id, data: { disabled: false } })}
                  icon={<Check weight="duotone" className="h-4 w-4" />}
                />
              ) : (
                <Button
                  size="icon"
                  variant="ghost"
                  className="hover:text-fg-danger"
                  aria-label={t('pages.account.users.disable')}
                  disabled={update.isPending}
                  onClick={() => setPendingDisable(u)}
                  icon={<Prohibit weight="duotone" className="h-4 w-4" />}
                />
              ))}
          </div>
        ),
      },
    ],
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [t, me?.id, update.isPending],
  )

  return (
    <Card>
      <CardHeader
        title={t('pages.account.users.title')}
        description={t('pages.account.users.description')}
        action={
          <Button
            size="sm"
            variant="primary"
            icon={<Plus weight="bold" className="h-4 w-4" />}
            onClick={() => setCreateOpen(true)}
          >
            {t('pages.account.users.add')}
          </Button>
        }
      />
      <CardBody className="p-0">
        {usersQuery.isError && !usersQuery.data ? (
          <div className="p-4">
            <ErrorState
              variant="inline"
              error={usersQuery.error}
              onRetry={() => usersQuery.refetch()}
              retrying={usersQuery.isFetching}
            />
          </div>
        ) : usersQuery.isPending ? (
          <SkeletonRows rows={3} columns={4} />
        ) : users.length === 0 ? (
          <EmptyState
            className="py-10"
            icon={<UserCircle weight="duotone" className="h-8 w-8" />}
            title={t('pages.account.users.title')}
            description={t('pages.account.users.description')}
          />
        ) : (
          <Table columns={columns} data={users} rowKey={(u) => u.id} dense />
        )}
      </CardBody>

      {/* Create account */}
      <Dialog
        open={createOpen}
        onClose={create.isPending ? () => undefined : () => setCreateOpen(false)}
        title={t('pages.account.users.add')}
        description={t('pages.account.users.createdHint')}
        footer={
          <>
            <Button variant="ghost" onClick={() => setCreateOpen(false)} disabled={create.isPending}>
              {t('common.cancel')}
            </Button>
            <Button
              variant="primary"
              loading={create.isPending}
              disabled={!email.trim() || password.length === 0}
              onClick={() =>
                create.mutate({
                  email: email.trim(),
                  password,
                  name: name.trim() || undefined,
                  role,
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
            label={t('pages.account.users.email')}
            value={email}
            autoFocus
            placeholder="ops@example.com"
            onChange={(e) => setEmail(e.target.value)}
            required
          />
          <Input
            label={t('pages.settings.displayName')}
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
          <Input
            type="password"
            label={t('pages.account.users.password')}
            value={password}
            autoComplete="new-password"
            hint={t('auth.passwordHint', { min: MIN_PASSWORD })}
            onChange={(e) => setPassword(e.target.value)}
            required
          />
          <Select
            label={t('pages.account.users.role')}
            value={role}
            options={USER_ROLES.map((r) => ({ value: r, label: t(`user.role.${r}`, r) }))}
            onChange={(e) => setRole(e.target.value)}
          />
        </div>
      </Dialog>

      {/* Edit account */}
      <Dialog
        open={editing !== null}
        onClose={update.isPending ? () => undefined : () => setEditing(null)}
        title={t('pages.account.users.edit')}
        description={editing?.email}
        footer={
          <>
            <Button variant="ghost" onClick={() => setEditing(null)} disabled={update.isPending}>
              {t('common.cancel')}
            </Button>
            <Button
              variant="primary"
              loading={update.isPending}
              disabled={editing === null}
              onClick={() => {
                if (!editing) return
                const isSelf = editing.id === me?.id
                update.mutate({
                  id: editing.id,
                  data: {
                    name: editName.trim(),
                    ...(isSelf ? {} : { role: editRole }),
                  },
                })
              }}
            >
              {t('common.save')}
            </Button>
          </>
        }
      >
        {editing && (
          <div className="flex flex-col gap-4">
            <Input
              label={t('pages.settings.displayName')}
              value={editName}
              onChange={(e) => setEditName(e.target.value)}
            />
            <Select
              label={t('pages.account.users.role')}
              value={editRole}
              disabled={editing.id === me?.id}
              hint={
                editing.id === me?.id
                  ? t('pages.account.users.selfRoleHint')
                  : undefined
              }
              options={USER_ROLES.map((r) => ({ value: r, label: t(`user.role.${r}`, r) }))}
              onChange={(e) => setEditRole(e.target.value)}
            />
          </div>
        )}
      </Dialog>

      <ConfirmDialog
        open={pendingDisable !== null}
        onClose={() => setPendingDisable(null)}
        onConfirm={() =>
          pendingDisable && update.mutate({ id: pendingDisable.id, data: { disabled: true } })
        }
        title={t('pages.account.users.disableTitle')}
        description={t('pages.account.users.disableDescription')}
        confirmLabel={t('pages.account.users.disable')}
        loading={update.isPending}
      >
        {pendingDisable && (
          <div className="rounded-md border border-line bg-recessed px-3 py-2">
            <p className="text-[13px] font-medium text-fg-strong">{pendingDisable.email}</p>
            {pendingDisable.name && (
              <p className="mt-0.5 text-xs text-fg-subtle">{pendingDisable.name}</p>
            )}
          </div>
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
