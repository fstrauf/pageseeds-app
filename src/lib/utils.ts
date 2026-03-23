import { clsx, type ClassValue } from 'clsx'
import { twMerge } from 'tailwind-merge'

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

export const STATUS_COLORS: Record<string, string> = {
  todo: 'bg-slate-700 text-slate-300',
  in_progress: 'bg-indigo-900/60 text-indigo-300',
  review: 'bg-amber-900/60 text-amber-300',
  done: 'bg-emerald-900/60 text-emerald-300',
  cancelled: 'bg-slate-800 text-slate-500',
}

export const PRIORITY_COLORS: Record<string, string> = {
  high: 'text-red-400',
  medium: 'text-amber-400',
  low: 'text-slate-400',
}

export const STATUS_NEXT: Record<string, string> = {
  todo: 'in_progress',
  in_progress: 'review',
  review: 'done',
}

export function formatDate(iso: string): string {
  if (!iso) return '—'
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return '—'

  return date.toLocaleString('en-US', {
    month: 'short',
    day: 'numeric',
    year: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
    hour12: false,
  })
}
