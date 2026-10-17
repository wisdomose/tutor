import { createFileRoute } from '@tanstack/react-router'
import { createServerFn } from '@tanstack/react-start'
import { Card, Label } from '#/components/ui'

type Health = { database: string; storage: string; claude: string }

// Runs on the web server, so the API address is read at runtime, not baked into the bundle.
const getApiHealth = createServerFn().handler(async () => {
  const apiUrl = process.env.API_URL ?? 'http://localhost:8080'
  try {
    const res = await fetch(`${apiUrl}/health`)
    return { apiUrl, health: (await res.json()) as Health, error: null }
  } catch (err) {
    return { apiUrl, health: null, error: String(err) }
  }
})

export const Route = createFileRoute('/')({
  loader: () => getApiHealth(),
  component: Home,
})

const checkNames: Record<keyof Health, string> = {
  database: 'Database',
  storage: 'Photo storage',
  claude: 'Claude',
}

function Home() {
  const { health, error } = Route.useLoaderData()
  const allOk = health && Object.values(health).every((s) => s.startsWith('ok'))

  return (
    <main className="mx-auto flex max-w-xl flex-col gap-4 px-4 pt-6 pb-12">
      <header className="pb-2">
        <Label>Tutor</Label>
        <h1 className="font-display text-[30px] leading-tight font-extrabold">
          Question converter
        </h1>
        <p className="mt-1 text-sm font-bold text-muted">
          Snap a page, Claude reads and answers it, you check the unsure ones.
        </p>
      </header>

      <Card fill={allOk ? 'lime' : 'lavender'}>
        <div className="mb-3 flex items-end justify-between">
          <div>
            <Label>Setup check</Label>
            <div className="font-display text-2xl leading-tight font-extrabold">
              {error ? 'API unreachable' : allOk ? 'All systems go' : 'Something needs attention'}
            </div>
          </div>
        </div>
        {error ? (
          <p className="rounded-[16px] border-[2.5px] border-ink bg-cream p-3 text-sm font-bold">
            {error}
          </p>
        ) : (
          <ul className="flex flex-col gap-2.5">
            {health &&
              (Object.keys(checkNames) as (keyof Health)[]).map((key) => {
                const ok = health[key].startsWith('ok')
                return (
                  <li
                    key={key}
                    className="flex items-center gap-3 rounded-[16px] border-[2.5px] border-ink bg-paper p-3"
                  >
                    <div
                      className={`flex size-9 shrink-0 items-center justify-center rounded-[11px] font-display text-lg font-extrabold ${
                        ok ? 'bg-ink text-lime' : 'border-[2.5px] border-ink bg-cream'
                      }`}
                    >
                      {ok ? '✓' : '!'}
                    </div>
                    <div className="min-w-0 flex-1">
                      <div className="text-[15px] font-extrabold">{checkNames[key]}</div>
                      <div className="truncate text-xs font-bold text-muted">{health[key]}</div>
                    </div>
                  </li>
                )
              })}
          </ul>
        )}
      </Card>
    </main>
  )
}
