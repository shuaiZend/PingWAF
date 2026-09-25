import { useMemo, useState } from 'react'
import { useParams } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  FileHtml,
  ArrowClockwise,
  ArrowsCounterClockwise,
  Code,
  Eye,
  BracketsCurly,
} from '@phosphor-icons/react'
import { PageHeader } from '@/components/PageHeader'
import { Card } from '@/components/ui/Card'
import { Button } from '@/components/ui/Button'
import { Input } from '@/components/ui/Input'
import { Select } from '@/components/ui/Select'
import { Switch } from '@/components/ui/Switch'
import { Badge } from '@/components/ui/Badge'
import { Dialog } from '@/components/ui/Dialog'
import { Textarea } from '@/components/ui/Textarea'
import { SkeletonCard } from '@/components/ui/Skeleton'
import { ConfirmDialog } from '@/components/ui/ConfirmDialog'
import { useToast } from '@/components/ui/Toast'
import { ErrorState } from '@/components/ErrorState'
import {
  errorPagesApi,
  errorPageKeys,
  ERROR_PAGE_VARIABLES,
  ERROR_PAGE_MESSAGES,
  defaultErrorPageTemplate,
  renderErrorPageTemplate,
} from '@/api/errorPages'
import { useCanWrite } from '@/hooks'
import { cn } from '@/lib/utils'
import {
  ERROR_PAGE_CONTENT_TYPES,
  ERROR_PAGE_STATUS_CODES,
  type ErrorPage,
  type ErrorPageContentType,
} from '@/api/types'

interface EditorState {
  status_code: number
  name: string
  content_type: ErrorPageContentType
  template: string
  enabled: boolean
  existingId: string | null
}

export function ErrorPagesPage() {
  const { t } = useTranslation()
  const toast = useToast()
  const queryClient = useQueryClient()
  const canWrite = useCanWrite()
  const { siteId = '' } = useParams<{ siteId: string }>()

  const [editor, setEditor] = useState<EditorState | null>(null)
  const [pendingReset, setPendingReset] = useState<EditorState | null>(null)
  const [pendingDelete, setPendingDelete] = useState<ErrorPage | null>(null)

  const pagesQuery = useQuery({
    queryKey: errorPageKeys.list(siteId),
    queryFn: () => errorPagesApi.list(siteId),
    enabled: Boolean(siteId),
  })

  const byStatus = useMemo(() => {
    const map = new Map<number, ErrorPage>()
    for (const p of pagesQuery.data ?? []) map.set(p.status_code, p)
    return map
  }, [pagesQuery.data])

  const invalidate = () => {
    void queryClient.invalidateQueries({ queryKey: errorPageKeys.all(siteId) })
  }

  const save = useMutation({
    mutationFn: (state: EditorState) =>
      state.existingId
        ? errorPagesApi.update(siteId, state.existingId, {
            status_code: state.status_code,
            name: state.name,
            content_type: state.content_type,
            template: state.template,
            enabled: state.enabled,
          })
        : errorPagesApi.upsert(siteId, {
            status_code: state.status_code,
            name: state.name,
            content_type: state.content_type,
            template: state.template,
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
      errorPagesApi.update(siteId, page.id, { enabled }),
    onSuccess: () => invalidate(),
  })

  const remove = useMutation({
    mutationFn: (id: string) => errorPagesApi.delete(siteId, id),
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
      content_type: (existing?.content_type as ErrorPageContentType) ?? 'html',
      template:
        existing?.template ??
        defaultErrorPageTemplate(statusCode, (existing?.content_type as ErrorPageContentType) ?? 'html'),
      enabled: existing?.enabled ?? true,
      existingId: existing?.id ?? null,
    })
  }

  const previewHtml = editor ? renderErrorPageTemplate(editor.template) : ''

  const cards = [...ERROR_PAGE_STATUS_CODES]

  return (
    <div className="animate-slide-up">
      <PageHeader
        title={t('pages.errorPages.title')}
        description={t('pages.errorPages.description')}
        actions={
          <Button
            variant="secondary"
            loading={pagesQuery.isFetching}
            onClick={() => pagesQuery.refetch()}
            icon={<ArrowClockwise weight="duotone" className="h-4 w-4" />}
          >
            {t('common.refresh')}
          </Button>
        }
      />

      {pagesQuery.isError && !pagesQuery.data ? (
        <ErrorState
          error={pagesQuery.error}
          onRetry={() => pagesQuery.refetch()}
          retrying={pagesQuery.isFetching}
        />
      ) : pagesQuery.isPending ? (
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {cards.map((c) => (
            <SkeletonCard key={c} />
          ))}
        </div>
      ) : (
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {cards.map((code) => {
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
                        {page?.content_type ?? 'html'}
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

      {/* Editor */}
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
                  label: c.toUpperCase(),
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
                  value={editor.template}
                  onChange={(e) => setEditor({ ...editor, template: e.target.value })}
                />
              </div>

              {/* Preview */}
              <div className="flex flex-col gap-1.5">
                <span className="flex items-center gap-1.5 text-[13px] font-medium text-fg">
                  <Eye weight="duotone" className="h-4 w-4" />
                  {t('pages.errorPages.preview')}
                </span>
                <div className="h-[388px] overflow-hidden rounded-md border border-line bg-white">
                  {editor.content_type === 'html' ? (
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
                          e ? { ...e, template: `${e.template}{{${v.name}}}` } : e,
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
            template: defaultErrorPageTemplate(
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
    </div>
  )
}

export default ErrorPagesPage
