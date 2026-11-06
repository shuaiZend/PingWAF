import { useTranslation } from 'react-i18next'
import { Card } from '@/components/ui/Card'
import { formatSize } from '@/lib/format'
import { cn } from '@/lib/utils'
import type { SystemMemoryMetrics } from '@/api/debug'

/**
 * 承诺内存告警阈值：≥85% danger、≥70% warning。
 *
 * 故意比 RSS 使用率（>75 黄 >90 红）更早告警：Committed_AS 是内核已向进程
 * 承诺的虚拟内存总量，比 RSS（实际驻留）先行增长 —— OOM killer 看
 * CommitLimit，不看 RSS。承诺打到 85% 时 RSS 可能还很低，留出反应窗口。
 */
export const COMMIT_WARNING_PERCENT = 70
export const COMMIT_DANGER_PERCENT = 85

function commitTone(percent: number | undefined): string {
  if (percent == null) return 'text-fg-strong'
  if (percent >= COMMIT_DANGER_PERCENT) return 'text-danger'
  if (percent >= COMMIT_WARNING_PERCENT) return 'text-warning'
  return 'text-fg-strong'
}

/** 单行明细：label + 右对齐值（可选副文案）。 */
function Row({
  label,
  hint,
  value,
  sub,
  valueTone,
}: {
  label: string
  hint: string
  value: string
  sub?: string
  valueTone?: string
}) {
  return (
    <div
      className="flex items-baseline justify-between gap-3 py-1"
      title={hint}
    >
      <span className="text-[13px] text-fg-subtle">{label}</span>
      <span className="flex items-baseline gap-2">
        {sub && (
          <span className={cn('text-[12px] tabular-nums', valueTone ?? 'text-fg-subtle')}>
            {sub}
          </span>
        )}
        <span className={cn('text-[13px] font-medium tabular-nums', valueTone ?? 'text-fg-strong')}>
          {value}
        </span>
      </span>
    </div>
  )
}

/**
 * 内存明细补充行：虚拟地址空间（VmSize）与系统承诺内存（Committed_AS）。
 *
 * VmSize 常远大于 RSS：大分配器 arena 会整块预留地址空间但不驻留物理页，
 * "RSS 小但 VmSize 大"是正常的预留行为。空值渲染 —。
 */
export function MemoryDetail({ memory }: { memory: SystemMemoryMetrics | undefined }) {
  const { t } = useTranslation()
  if (!memory) return null
  const vmSize = memory.vm_size ?? formatSize(memory.vm_size_bytes)
  const committed = memory.committed_as ?? formatSize(memory.committed_as_bytes)
  const commitPercent =
    memory.commit_percent != null ? memory.commit_percent.toFixed(1) : null
  const tone = commitTone(memory.commit_percent)
  return (
    <Card className="p-4">
      <div className="flex flex-col">
        <Row
          label={t('system.memory.vmSize')}
          hint={t('system.memory.vmSizeHint')}
          value={vmSize}
        />
        <Row
          label={t('system.memory.commit')}
          hint={t('system.memory.commitHint')}
          value={committed}
          sub={commitPercent ? `${commitPercent}%` : undefined}
          valueTone={tone}
        />
        <div className="flex items-baseline justify-end gap-2 py-1">
          <span className="text-[12px] text-fg-subtle">{t('system.memory.commitLimit')}</span>
          <span className="text-[12px] tabular-nums text-fg-strong">
            {memory.commit_limit ?? formatSize(memory.commit_limit_bytes)}
          </span>
        </div>
      </div>
    </Card>
  )
}
