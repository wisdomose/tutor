import { createFileRoute } from '@tanstack/react-router'
import { createServerFn } from '@tanstack/react-start'

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

function Home() {
  const { apiUrl, health, error } = Route.useLoaderData()
  return (
    <main className="mx-auto max-w-xl p-6">
      <h1 className="text-3xl font-bold">Tutor</h1>
      <p className="mt-2 text-gray-600">Question converter — setup check</p>
      <h2 className="mt-6 font-semibold">API ({apiUrl})</h2>
      {error ? (
        <p className="mt-2 text-red-600">Unreachable: {error}</p>
      ) : (
        <ul className="mt-2 space-y-1">
          {health &&
            Object.entries(health).map(([name, status]) => (
              <li key={name}>
                <span className={status.startsWith('ok') ? 'text-green-700' : 'text-red-600'}>
                  {status.startsWith('ok') ? '✓' : '✗'}
                </span>{' '}
                <strong>{name}</strong>: {status}
              </li>
            ))}
        </ul>
      )}
    </main>
  )
}
