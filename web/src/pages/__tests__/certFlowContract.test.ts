import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'

const read = (path: string) => readFileSync(resolve(process.cwd(), path), 'utf8')

/**
 * 证书「上传 / 绑定 / 解绑 / 删除」UI 契约：绑定弹窗三态、逐站批量结果、
 * 409 引导与列表口径。断言源码而非运行时，防止回归无声漂移。
 */
describe('certificate bind / unbind / delete UI contract', () => {
  const page = read('src/pages/GlobalSslPage.tsx')
  const sslApi = read('src/api/ssl.ts')

  it('delegates the three flow decisions to the pure helpers in lib/certFlow', () => {
    expect(page).toMatch(/import \{[^}]*bindingKind[^}]*deleteBlockers[^}]*mergeBindResults[^}]*\} from '(@\/lib\/certFlow|\.\.\/lib\/certFlow)'/)
    expect(page).toContain('bindingKind(')
    expect(page).toContain('mergeBindResults(')
    expect(page).toContain('deleteBlockers(')
    // 旧的内联判定不应残留。
    expect(page).not.toContain('bound_sites?.some(')
    expect(page).not.toContain('deleteUsage.serving_sites.length > 0 || deleteUsage.linked_sites.length > 0')
  })

  it('usage endpoint is wired: GET /certificates/{id}/usage with a cache key', () => {
    expect(sslApi).toContain("apiClient.get<CertificateUsage>(`/certificates/${certId}/usage`)")
    expect(sslApi).toContain("usage: (certId: string) => [...sslKeys.all, 'usage', certId] as const")
    expect(page).toContain('sslApi.usage(')
  })

  it('bind dialog rows use three states: owner badge, bound unbind, fresh checkbox', () => {
    // 属主行：Badge「属主」+ checkbox 禁用，且该分支不渲染解绑按钮。
    const ownerBranch = page.slice(
      page.indexOf('{owner ? ('),
      page.indexOf(') : linked ? ('),
    )
    expect(ownerBranch).toContain("t('pages.sslGlobal.ownerBadge')")
    expect(ownerBranch).not.toContain('pages.sslGlobal.unbind')
    // 属主/已绑定的 checkbox 都不可勾选。
    expect(page).toMatch(/const checked = owner \|\| linked \|\| bindSelection/)
    expect(page).toMatch(/disabled=\{!canWrite \|\| bindCert\.isPending \|\| owner \|\| linked\}/)
    // 已绑定行经二次确认后再解绑。
    expect(page).toContain('setPendingUnbind({ certId: bindTarget.id, siteId: s.id })')
    expect(page).toContain("t('pages.sslGlobal.unbindConfirmDescription')")
  })

  it('dialog header shows the owner line and the share count (or 尚未共享)', () => {
    expect(page).toContain("t('pages.sslGlobal.ownerLine', {")
    expect(page).toContain("t('pages.sslGlobal.sharedCount', {")
    expect(page).toContain("t('pages.sslGlobal.notShared')")
    // usage 拿不到时回退 bound_sites / bound_count。
    expect(page).toContain('bindTarget.bound_count')
  })

  it('batch bind collects per-site results serially and never aborts on the first failure', () => {
    // 逐站提交：串行 for-of（每站触发一次配置递增/agent 推送），每站
    // try/catch 收集 BindAttempt，失败不中断批量。
    expect(page).toContain('interface BindAttempt {')
    expect(page).toContain('ok: boolean')
    expect(page).toMatch(/for \(const siteId of ids\) \{\s*\n\s*try \{/)
    expect(page).not.toContain('Promise.all')
    // 有失败：保持打开 + 结果区列出「域名 — 原因」；全部成功才关闭。
    expect(page).toContain('failures.length === 0')
    expect(page).toContain("t('pages.sslGlobal.batchFailed'")
    expect(page).toContain("t('pages.sslGlobal.bindSelectedCount', { count: bindSelection.length })")
  })

  it('unbind 409 renders an in-place hint linking to /sites/{id}/ssl', () => {
    expect(page).toContain('setUnbindConflictSite(siteId)')
    expect(page).toContain("t('pages.sslGlobal.unbindInUse')")
    expect(page).toMatch(/navigate\(`\/sites\/\$\{s\.id\}\/ssl`\)/)
  })

  it('delete 409 guidance: blocked confirm navigates to the first blocking site', () => {
    // 保守删除保护：deleteBlockers 在 usage 不可用时返回 unknown+blocked。
    expect(page).toMatch(/const deleteGuard = deleteBlockers\(/)
    expect(page).toMatch(/const deleteBlocked = pendingDelete === null \|\| deleteGuard\.blocked/)
    expect(page).toMatch(/deleteUsageQuery\.isError/)
    expect(page).toContain("t('pages.sslGlobal.usageUnavailable')")
    // blocked 分支的确认是跳转，不是删除；按钮不被 disabled，loading 仍生效。
    expect(page).toMatch(/onConfirm=\{\(\) => \{\s*\n\s*if \(!pendingDelete\) return\s*\n\s*if \(deleteHasBlockingSites\) \{/)
    expect(page).toMatch(/navigate\(`\/sites\/\$\{firstBlockingSite\.id\}\/ssl`\)/)
    expect(page).not.toContain('disabled={deleteBlocked}')
    expect(page).toContain('disabled={deleteCert.isPending}')
    expect(page).toContain("t('pages.sslGlobal.goHandle')")
    expect(page).toContain("t('pages.sslGlobal.deleteBlockedServing')")
    expect(page).toContain("t('pages.sslGlobal.deleteBlockedLinked')")
    expect(page).toMatch(/navigate\(`\/sites\/\$\{site\.id\}\/ssl`\)/)
  })

  it('delete confirm has three branches: go-handle / close (unknown) / delete', () => {
    // 分支判定用 reason，而不是 sites.length 猜测。
    expect(page).toMatch(
      /deleteHasBlockingSites =\s*\n\s*deleteGuard\.reason === 'serving' \|\| deleteGuard\.reason === 'linked'/,
    )
    // 有具体阻塞站点 → 「前往处理」+ 跳转。
    expect(page).toMatch(/deleteHasBlockingSites\s*\n\s*\? t\('pages\.sslGlobal\.goHandle'\)/)
    // unknown（usage 未返回/失败）→ 按钮是关闭，onConfirm 只关弹窗、不含 navigate。
    expect(page).toContain("? t('common.close')")
    expect(page).toContain('deleteCert.mutate(pendingDelete.id)')
    // unknown 分支不渲染空的阻塞站点警示框。
    expect(page).not.toMatch(/\{deleteBlocked && \(/)
  })

  it('batch failure reason falls back to an i18n key instead of hardcoded English', () => {
    // 纯函数无 i18n：reason 缺失时为空串，页面渲染时兜底 i18n key。
    const flow = read('src/lib/certFlow.ts')
    expect(flow).not.toContain("'unknown error'")
    expect(flow).toContain('r.error ?? \'\''  )
    expect(page).toContain("r.reason || t('pages.sslGlobal.batchResult')")
  })

  it('list rows: 属主：<domain> / 未绑定, second line shared count, unbound badge only when truly unbound', () => {
    expect(page).toContain('r.site_id === null && (r.bound_count ?? 0) === 0')
    expect(page).toContain("t('pages.sslGlobal.ownerLine', { domain: r.site_domain ?? '—' })")
    expect(page).toContain("t('pages.sslGlobal.sharedCount', { count: r.bound_count })")
  })
})

describe('site SSL page shared-certificate contract', () => {
  const src = read('src/pages/sites/SslPage.tsx')

  it('linked rows keep the renew button disabled with an owner-renewal tooltip', () => {
    expect(src).toContain('c.relation === \'linked\' ? t(\'pages.ssl.renewSharedHint\')')
    expect(src).toMatch(/disabled=\{!canWrite \|\| renew\.isPending \|\| c\.relation === 'linked'\}/)
  })

  it('remove-share 409 highlights the clear-certificate button only while that cert stays selected', () => {
    expect(src).toContain('e instanceof ApiError && e.status === 409')
    // 派生值：冲突证书被切换（save/clear）后红色高亮自动消失。
    expect(src).toContain('unlinkConflictCertId !== null && unlinkConflictCertId === drafts?.https.certificate_id')
    expect(src).toContain('setUnlinkConflictCertId(certId)')
    expect(src).toContain('setUnlinkConflictCertId(null)')
    expect(src).toContain('clearCertRef.current?.scrollIntoView(')
    expect(src).toContain("t('pages.ssl.clearFirst')")
    expect(src).toContain('ring-2 ring-danger/60')
  })
})

describe('certificate flow i18n keys exist in every language', () => {
  it('sslGlobal + ssl keys are complete across en/zh/ja', () => {
    for (const lang of ['en', 'zh', 'ja']) {
      const parsed = JSON.parse(
        read(`src/i18n/locales/${lang}/common.json`),
      ) as { pages: { sslGlobal: Record<string, string>; ssl: Record<string, string> } }
      for (const key of [
        'ownerBadge', 'ownerLine', 'notShared', 'sharedCount', 'batchResult',
        'batchFailed', 'goToSiteSsl', 'unbindInUse', 'deleteBlockedServing',
        'deleteBlockedLinked', 'goHandle', 'usageUnavailable',
      ]) {
        expect(parsed.pages.sslGlobal[key], `${lang}.sslGlobal.${key}`).toBeTruthy()
      }
      expect(parsed.pages.ssl.renewSharedHint, `${lang}.ssl.renewSharedHint`).toBeTruthy()
      expect(parsed.pages.ssl.clearFirst, `${lang}.ssl.clearFirst`).toBeTruthy()
    }
  })
})
