import { toast } from 'sonner'

export function toastSuccess(message: string, description?: string) {
  toast.success(message, description ? { description } : undefined)
}

export function toastError(message: string, description?: string) {
  toast.error(message, description ? { description } : undefined)
}

export function toastInfo(message: string, description?: string) {
  toast.info(message, description ? { description } : undefined)
}
