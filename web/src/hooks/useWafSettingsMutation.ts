import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { useToast } from '@/components/ui/Toast'
import { wafSettingsApi, wafSettingsKeys } from '@/api/wafSettings'
import type { UpdateWafSettingsRequest, WafSettings } from '@/api/types'

/**
 * Site WAF settings update with optimistic patch semantics, shared by every
 * protection panel that flips a grading knob: the cache is patched in-place
 * before the PUT leaves, rolled back on failure and re-synced on success.
 */
export function useWafSettingsMutation(siteId: string) {
  const { t } = useTranslation()
  const toast = useToast()
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: (payload: UpdateWafSettingsRequest) =>
      wafSettingsApi.update(siteId, payload),
    onMutate: async (payload) => {
      await queryClient.cancelQueries({
        queryKey: wafSettingsKeys.settings(siteId),
      })
      const previous = queryClient.getQueryData<WafSettings>(
        wafSettingsKeys.settings(siteId),
      )
      queryClient.setQueryData<WafSettings>(
        wafSettingsKeys.settings(siteId),
        (prev) => (prev ? { ...prev, ...payload } : prev),
      )
      return { previous }
    },
    onError: (error, _payload, context) => {
      if (context?.previous) {
        queryClient.setQueryData(
          wafSettingsKeys.settings(siteId),
          context.previous,
        )
      }
      toast.error(
        t('pages.protection.updateFailed'),
        error instanceof Error ? error.message : undefined,
      )
    },
    onSuccess: (saved) => {
      queryClient.setQueryData(wafSettingsKeys.settings(siteId), saved)
      toast.success(t('pages.protection.saved'))
    },
    onSettled: () => {
      void queryClient.invalidateQueries({
        queryKey: wafSettingsKeys.all(siteId),
      })
    },
  })
}
