// Sticker-style building blocks. See docs/design/README.md.
import type { ButtonHTMLAttributes, ReactNode } from 'react'

const fills = {
  lime: 'bg-lime',
  paper: 'bg-paper',
  lavender: 'bg-lavender',
  'lavender-soft': 'bg-lavender-soft',
  cream: 'bg-cream',
} as const
type Fill = keyof typeof fills

const cx = (...parts: (string | false | undefined)[]) => parts.filter(Boolean).join(' ')

/** Ink base with the coloured face lifted 4px; the face drops onto the base when pressed. */
export function Button({
  fill = 'lime',
  size = 'md',
  className,
  children,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { fill?: Fill; size?: 'sm' | 'md' }) {
  const radius = size === 'sm' ? 'rounded-[13px]' : 'rounded-[16px]'
  return (
    <button
      type="button"
      {...props}
      className={cx('group mt-1 bg-ink disabled:opacity-50', radius, className)}
    >
      <span
        className={cx(
          'block -translate-y-1 whitespace-nowrap border-[2.5px] border-ink font-display font-extrabold text-ink transition-transform duration-75 group-active:translate-y-0 group-disabled:translate-y-0',
          radius,
          fills[fill],
          size === 'sm' ? 'px-3.5 py-2 text-sm' : 'px-5 py-3.5 text-lg',
        )}
      >
        {children}
      </span>
    </button>
  )
}

export function Card({
  fill = 'paper',
  className,
  children,
}: {
  fill?: Fill
  className?: string
  children: ReactNode
}) {
  return (
    <div className={cx('rounded-[18px] border-[2.5px] border-ink p-4', fills[fill], className)}>
      {children}
    </div>
  )
}

/** Small uppercase outlined tag, e.g. HAS DIAGRAM. */
export function Chip({ fill = 'cream', children }: { fill?: Fill; children: ReactNode }) {
  return (
    <span
      className={cx(
        'inline-block rounded-[10px] border-2 border-ink px-2 py-0.5 text-[11px] font-extrabold uppercase tracking-wide',
        fills[fill],
      )}
    >
      {children}
    </span>
  )
}

/** Ink pill with lime text: location, counts, timers. */
export function Badge({ children }: { children: ReactNode }) {
  return (
    <span className="inline-block rounded-[8px] bg-ink px-2.5 py-0.5 text-[11px] font-extrabold uppercase tracking-wide text-lime">
      {children}
    </span>
  )
}

export function Label({ children }: { children: ReactNode }) {
  return <div className="text-xs font-extrabold uppercase tracking-[1.2px]">{children}</div>
}

/** Option row: letter tile + text, with the answer-revealed states. */
export function OptionRow({
  label,
  children,
  state = 'idle',
}: {
  label: string
  children: ReactNode
  state?: 'idle' | 'correct' | 'dimmed'
}) {
  const correct = state === 'correct'
  return (
    <div
      className={cx(
        'flex items-center gap-3.5 rounded-[18px] border-ink p-3.5',
        correct ? 'border-[3px] bg-lime' : 'border-[2.5px] bg-paper',
        state === 'dimmed' && 'bg-cream opacity-45',
      )}
    >
      <div
        className={cx(
          'flex size-[34px] shrink-0 items-center justify-center rounded-[11px] font-display text-base font-extrabold',
          correct ? 'bg-ink text-lime' : 'border-[2.5px] border-ink bg-lavender-soft',
        )}
      >
        {label}
      </div>
      <div className={cx('flex-1 text-lg', correct ? 'font-extrabold' : 'font-bold')}>{children}</div>
      {correct && <Chip fill="paper">Correct</Chip>}
    </div>
  )
}
