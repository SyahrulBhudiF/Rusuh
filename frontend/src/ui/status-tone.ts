export const STATUS_TONE: Record<string, string> = {
  active: 'border-emerald-500/35 bg-emerald-500/15 text-emerald-700 dark:text-emerald-300',
  refreshing: 'border-sky-500/35 bg-sky-500/15 text-sky-700 dark:text-sky-300',
  pending: 'border-amber-500/35 bg-amber-500/15 text-amber-700 dark:text-amber-300',
  error:
    'border-destructive/40 bg-destructive/10 text-destructive dark:text-destructive-foreground',
  disabled: 'border-border bg-muted text-muted-foreground',
  unknown: 'border-border bg-muted text-muted-foreground',
}
export function statusTone(status: string) {
  return STATUS_TONE[status] ?? 'border-border bg-muted text-muted-foreground'
}
