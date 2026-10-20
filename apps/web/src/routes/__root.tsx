import { TanStackDevtools } from "@tanstack/react-devtools";
import {
	QueryClient,
	QueryClientProvider,
	useQuery,
} from "@tanstack/react-query";
import {
	createRootRoute,
	HeadContent,
	Link,
	Scripts,
	useRouterState,
} from "@tanstack/react-router";
import { TanStackRouterDevtoolsPanel } from "@tanstack/react-router-devtools";
import { useEffect, useState } from "react";
import { ToastProvider } from "#/components/Toast";
import { cx } from "#/components/ui";
import { api, unwrap } from "#/lib/api";
import appCss from "../styles.css?url";

export const Route = createRootRoute({
	head: () => ({
		meta: [
			{ charSet: "utf-8" },
			{ name: "viewport", content: "width=device-width, initial-scale=1" },
			{ title: "Tutor" },
		],
		links: [
			{ rel: "preconnect", href: "https://fonts.googleapis.com" },
			{
				rel: "preconnect",
				href: "https://fonts.gstatic.com",
				crossOrigin: "anonymous",
			},
			{
				rel: "stylesheet",
				href: "https://fonts.googleapis.com/css2?family=Baloo+2:wght@500;600;700;800&family=Nunito:wght@400;600;700;800&display=swap",
			},
			{ rel: "stylesheet", href: appCss },
		],
	}),
	shellComponent: RootDocument,
});

function RootDocument({ children }: { children: React.ReactNode }) {
	const [queryClient] = useState(
		() =>
			new QueryClient({
				defaultOptions: { queries: { staleTime: 5_000, retry: 1 } },
			}),
	);
	return (
		<html lang="en">
			<head>
				<HeadContent />
			</head>
			<body>
				<QueryClientProvider client={queryClient}>
					<ToastProvider>
						<Nav />
						<main className="mx-auto flex max-w-3xl flex-col gap-4 px-4 pt-4 pb-24">
							{children}
						</main>
						<QueueFab />
					</ToastProvider>
				</QueryClientProvider>
				{import.meta.env.DEV && (
					<TanStackDevtools
						config={{ position: "bottom-left" }}
						plugins={[
							{
								name: "Tanstack Router",
								render: <TanStackRouterDevtoolsPanel />,
							},
						]}
					/>
				)}
				<Scripts />
			</body>
		</html>
	);
}

// Uploading happens on the home page (the Tutor logo).
const links = [
	{ to: "/uploads", label: "Uploads" },
	{ to: "/review", label: "Review" },
	{ to: "/questions", label: "Questions" },
] as const;

function Nav() {
	const review = useQuery({
		queryKey: ["review", null],
		queryFn: () => unwrap(api.GET("/review", {})),
		refetchInterval: 15_000,
	});
	const toCheck = review.data?.total ?? 0;
	const [open, setOpen] = useState(false);
	// Close the phone menu whenever the page changes.
	const path = useRouterState({ select: (s) => s.location.pathname });
	useEffect(() => {
		if (path) setOpen(false);
	}, [path]);

	const linkList = (stacked: boolean) =>
		links.map((l) => (
			<Link
				key={l.to}
				to={l.to}
				className={cx(
					"flex items-center gap-1.5 rounded-[11px] border-2 border-transparent font-extrabold whitespace-nowrap",
					stacked ? "px-3 py-2.5 text-base" : "px-2.5 py-1 text-sm",
				)}
				activeProps={{ className: "border-ink! bg-lime" }}
			>
				{l.label}
				{l.to === "/review" && toCheck > 0 && (
					<span className="rounded-[7px] bg-ink px-1.5 text-[11px] text-lime">
						{toCheck}
					</span>
				)}
			</Link>
		));

	return (
		<nav className="sticky top-0 z-10 border-b-[2.5px] border-ink bg-cream">
			<div className="mx-auto flex max-w-3xl items-center gap-2 px-4 py-2.5">
				<Link to="/" className="mr-1 font-display text-xl font-extrabold">
					Tutor
				</Link>
				<div className="hidden gap-1 sm:flex">{linkList(false)}</div>
				<span className="flex-1" />
				<button
					type="button"
					className="flex size-10 items-center justify-center rounded-[13px] border-[2.5px] border-ink bg-paper sm:hidden"
					aria-label={open ? "Close menu" : "Open menu"}
					aria-expanded={open}
					onClick={() => setOpen((o) => !o)}
				>
					<span className="relative block h-3.5 w-5">
						{open ? (
							<>
								<span className="absolute top-1/2 left-0 h-[3px] w-5 -translate-y-1/2 rotate-45 rounded bg-ink" />
								<span className="absolute top-1/2 left-0 h-[3px] w-5 -translate-y-1/2 -rotate-45 rounded bg-ink" />
							</>
						) : (
							<>
								<span className="absolute top-0 left-0 h-[3px] w-5 rounded bg-ink" />
								<span className="absolute top-1/2 left-0 h-[3px] w-5 -translate-y-1/2 rounded bg-ink" />
								<span className="absolute bottom-0 left-0 h-[3px] w-5 rounded bg-ink" />
							</>
						)}
						{!open && toCheck > 0 && (
							<span className="absolute -top-2.5 -right-3 size-3 rounded-full border-2 border-ink bg-lime" />
						)}
					</span>
				</button>
			</div>
			{open && (
				<div className="flex flex-col gap-1 border-t-[2.5px] border-ink bg-lavender-soft px-4 py-3 sm:hidden">
					{linkList(true)}
				</div>
			)}
		</nav>
	);
}

/**
 * Pages waiting to be read, as a floating button that opens Uploads. Hidden when nothing
 * is waiting; the ring spins while pages are being read and stops when the queue is paused.
 */
function QueueFab() {
	const queue = useQuery({
		queryKey: ["queue"],
		queryFn: () => unwrap(api.GET("/queue")),
		refetchInterval: (q) =>
			q.state.data && q.state.data.queued + q.state.data.processing > 0
				? 5_000
				: 15_000,
	});
	const q = queue.data;
	const pending = q ? q.queued + q.processing : 0;
	if (!q || pending === 0) return null;
	const paused = q.paused || !!q.limited_until;
	return (
		<Link
			to="/uploads"
			aria-label={`${pending} page${pending === 1 ? "" : "s"} ${paused ? "waiting, paused" : "being read"}`}
			className="fixed right-4 bottom-[max(1rem,env(safe-area-inset-bottom))] z-20 flex size-16 items-center justify-center"
		>
			<svg
				aria-hidden="true"
				viewBox="0 0 64 64"
				className={cx("absolute inset-0 size-16", !paused && "animate-spin")}
			>
				<circle
					cx="32"
					cy="32"
					r="29"
					fill="none"
					stroke="var(--color-paper)"
					strokeWidth="5"
				/>
				<circle
					cx="32"
					cy="32"
					r="29"
					fill="none"
					stroke="var(--color-ink)"
					strokeWidth="5"
					strokeLinecap="round"
					strokeDasharray={paused ? "182 0" : "60 122"}
				/>
			</svg>
			<span
				className={cx(
					"flex size-12 items-center justify-center rounded-full border-[2.5px] border-ink font-display text-lg leading-none font-extrabold",
					paused ? "bg-lavender" : "bg-lime",
				)}
			>
				{pending > 9 ? "9+" : pending}
			</span>
		</Link>
	);
}
