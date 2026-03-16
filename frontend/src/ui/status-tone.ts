export const STATUS_TONE: Record<string, string> = {
  active: 'bg-emerald-500/15 text-emerald-300 border-emerald-500/20',
  refreshing: 'bg-blue-500/15 text-blue-300 border-blue-500/20',
  pending: 'bg-amber-500/15 text-amber-300 border-amber-500/20',
  error: 'bg-red-500/15 text-red-300 border-red-500/20',
  disabled: 'bg-zinc-500/15 text-zinc-300 border-zinc-500/20',
}

export function statusTone(status: string) {
  return STATUS_TONE[status] ?? 'bg-white/10 text-[var(--muted-foreground)] border-[var(--border)]'
}
