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

  it('batch bind collects per-site results and never aborts on the first failure', () => {
    // 逐站提交：Promise.all + 每站 try/catch 收集 BindAttempt，而非顺序 await。
    expect(page).toContain('interface BindAttempt {')
    expect(page).toContain('ok: boolean')
    expect(page).toMatch(/await Promise\.all\(\s*\n\s*ids\.map\(async \(siteId\): Promise<BindAttempt>/)
    expect(page).not.toMatch(/for \(const siteId of ids\) \{\s*\n\s*await/)
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

  it('delete 409 guidance: blocked confirm button and clickable site list', () => {
    expect(page).toMatch(
      /deleteUsage\.serving_sites\.length > 0 \|\| deleteUsage\.linked_sites\.length > 0/,
    )
    expect(page).toContain('disabled={deleteBlocked}')
    expect(page).toContain("t('pages.sslGlobal.goHandle')")
    expect(page).toContain("t('pages.sslGlobal.deleteBlockedServing')")
    expect(page).toContain("t('pages.sslGlobal.deleteBlockedLinked')")
    expect(page).toMatch(/navigate\(`\/sites\/\$\{site\.id\}\/ssl`\)/)
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

  it('remove-share 409 scrolls to and highlights the clear-certificate button', () => {
    expect(src).toContain('e instanceof ApiError && e.status === 409')
    expect(src).toContain('setUnlinkConflict(true)')
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
        'deleteBlockedLinked', 'goHandle',
      ]) {
        expect(parsed.pages.sslGlobal[key], `${lang}.sslGlobal.${key}`).toBeTruthy()
      }
      expect(parsed.pages.ssl.renewSharedHint, `${lang}.ssl.renewSharedHint`).toBeTruthy()
      expect(parsed.pages.ssl.clearFirst, `${lang}.ssl.clearFirst`).toBeTruthy()
    }
  })
})
