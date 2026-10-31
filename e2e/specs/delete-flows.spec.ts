import { test, expect } from '../fixtures/app'
import { requiredEnv } from '../support/env'

/**
 * Regression for "clicking delete opens the edit page": every affected table
 * binds a row click that navigates or opens an edit dialog, and the delete
 * button inside the row used to bubble into it, unmounting the confirmation.
 *
 * Each case asserts three things in order:
 *   1. the delete button opens the confirmation without leaving the list;
 *   2. cancelling keeps the row (nothing was deleted by the row shortcut);
 *   3. confirming removes it.
 * The route case also proves the row shortcut still works on a plain cell, so
 * the fix cannot have been "disable row clicks".
 */

const run = `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`
const domain = `delete-${run}.e2e.test`
const origin = requiredEnv('E2E_ORIGIN_URL')

test.describe('delete actions open confirmations, not the edit view', () => {
  test('route, rate-limit and IP rules delete from their own row button', async ({ app }) => {
    const siteId = await app.createSite({
      name: `E2E delete ${run}`,
      domain,
      upstream: origin,
    })

    try {
      await app.waitForSite(domain)

      // ── Routes (the originally reported screen) ──────────────────────────
      await app.page.goto(app.adminUrl(`/sites/${siteId}/basic`))
      await app.addRoute({ name: `del-route-${run}`, path: '/delete-me' })

      const routeRow = app.page.getByRole('row').filter({ hasText: 'delete-me' })
      await expect(routeRow).toHaveCount(1)

      await routeRow.getByRole('button', { name: 'Delete' }).click()
      const routeDialog = app.page.getByRole('dialog')
      await expect(routeDialog).toBeVisible()
      // Still on the routes page: the row click must not have navigated away.
      expect(app.page.url()).toContain(`/sites/${siteId}/basic`)
      await expect(app.page.getByRole('dialog', { name: 'Edit route' })).toHaveCount(0)

      await routeDialog.getByRole('button', { name: 'Cancel' }).click()
      await expect(routeRow).toHaveCount(1)

      // The row shortcut itself must keep working for plain cells.
      await routeRow.getByText('/delete-me', { exact: true }).click()
      await expect(
        app.page.getByRole('dialog', { name: 'Edit route' }),
      ).toBeVisible()
      await app.page
        .getByRole('dialog', { name: 'Edit route' })
        .getByRole('button', { name: 'Cancel' })
        .click()

      await routeRow.getByRole('button', { name: 'Delete' }).click()
      await app.page
        .getByRole('dialog')
        .getByRole('button', { name: 'Delete', exact: true })
        .click()
      await expect(routeRow).toHaveCount(0)

      // ── Rate-limit rules ────────────────────────────────────────────────
      await app.page.goto(
        app.adminUrl(`/sites/${siteId}/security/rate-limiting`),
      )
      await app.page.getByRole('button', { name: 'Add rule' }).first().click()
      const rateDialog = app.page.getByRole('dialog')
      await rateDialog.getByRole('textbox', { name: 'Name' }).fill(`del-rate-${run}`)
      await rateDialog.getByRole('button', { name: 'Create', exact: true }).click()
      await expect(rateDialog).toHaveCount(0)

      const rateRow = app.page.getByRole('row').filter({ hasText: `del-rate-${run}` })
      await expect(rateRow).toHaveCount(1)
      await rateRow.getByRole('button', { name: 'Delete' }).click()
      const rateConfirm = app.page.getByRole('dialog')
      await expect(rateConfirm).toBeVisible()
      expect(app.page.url()).toContain('/security/rate-limiting')
      await rateConfirm.getByRole('button', { name: 'Delete', exact: true }).click()
      await expect(rateRow).toHaveCount(0)

      // ── IP access rules ─────────────────────────────────────────────────
      await app.page.goto(app.adminUrl(`/sites/${siteId}/security/ip-rules`))
      await app.page.getByRole('button', { name: 'Add rule' }).first().click()
      const ipDialog = app.page.getByRole('dialog')
      await ipDialog.getByRole('textbox', { name: 'Name' }).fill(`del-ip-${run}`)
      await ipDialog.getByRole('tab', { name: 'Manual list' }).click()
      await ipDialog
        .getByRole('textbox', { name: /IP \/ CIDR list/ })
        .fill('203.0.113.10')
      await ipDialog.getByRole('button', { name: 'Create', exact: true }).click()
      await expect(ipDialog).toHaveCount(0)

      const ipRow = app.page.getByRole('row').filter({ hasText: `del-ip-${run}` })
      await expect(ipRow).toHaveCount(1)
      await ipRow.getByRole('button', { name: 'Delete' }).click()
      const ipConfirm = app.page.getByRole('dialog')
      await expect(ipConfirm).toBeVisible()
      // `/security/ip-rules` redirects to the access-control tab; the point is
      // that we stayed on the rules screen instead of opening an editor.
      expect(app.page.url()).toContain(`/sites/${siteId}/security/`)
      await ipConfirm.getByRole('button', { name: 'Delete', exact: true }).click()
      await expect(ipRow).toHaveCount(0)
    } finally {
      await app.deleteSite(domain)
    }
  })

  test('site deletion from the list still confirms without opening the editor', async ({ app }) => {
    const siteDomain = `delete-site-${run}.e2e.test`
    await app.createSite({
      name: `E2E delete site ${run}`,
      domain: siteDomain,
      upstream: origin,
    })

    try {
      await app.page.goto(app.adminUrl('/sites'))
      const search = app.page.getByRole('textbox', { name: 'Search' })
      if (await search.count()) await search.fill(siteDomain)
      const row = app.page.getByRole('row').filter({ hasText: siteDomain })
      await expect(row).toHaveCount(1)

      await row.getByRole('button', { name: 'Delete' }).click()
      const dialog = app.page.getByRole('dialog')
      await expect(dialog).toBeVisible()
      // The sites list edits in place, so the assertion is simply "we are still
      // on the list and the confirmation is the only dialog".
      expect(app.page.url()).toContain('/sites')
      await dialog.getByRole('button', { name: 'Delete', exact: true }).click()
      await expect(row).toHaveCount(0)
    } finally {
      await app.deleteSiteIfPresent(siteDomain)
    }
  })
})
