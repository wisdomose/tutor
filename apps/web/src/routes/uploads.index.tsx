import { useQuery } from "@tanstack/react-query";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { PageRow } from "#/components/PageRow";
import { QueuePanel } from "#/components/QueuePanel";
import {
	Card,
	Empty,
	ErrorNote,
	Field,
	Label,
	PageTitle,
	Select,
	Spinner,
} from "#/components/ui";
import { api, unwrap } from "#/lib/api";

type Search = { subject?: string; status?: string };

export const Route = createFileRoute("/uploads/")({
	validateSearch: (s: Record<string, unknown>): Search => ({
		subject: typeof s.subject === "string" ? s.subject : undefined,
		status: typeof s.status === "string" ? s.status : undefined,
	}),
	component: UploadsPage,
});

const statuses = [
	{ value: "queued", label: "Waiting" },
	{ value: "processing", label: "Reading" },
	{ value: "done", label: "Done" },
	{ value: "failed", label: "Failed" },
];

function UploadsPage() {
	const search = Route.useSearch();
	const navigate = useNavigate({ from: Route.fullPath });
	const subjects = useQuery({
		queryKey: ["subjects"],
		queryFn: () => unwrap(api.GET("/subjects")),
	});
	const pages = useQuery({
		queryKey: ["pages", search],
		queryFn: () =>
			unwrap(
				api.GET("/pages", {
					params: {
						query: {
							subject_id: search.subject,
							status: search.status,
							limit: 200,
						},
					},
				}),
			),
		refetchInterval: 5_000,
	});

	return (
		<>
			<PageTitle>Uploads</PageTitle>
			<QueuePanel />

			<div className="grid grid-cols-2 gap-3">
				<Field label="Subject">
					<Select
						value={search.subject ?? ""}
						onChange={(e) =>
							navigate({
								search: (s) => ({ ...s, subject: e.target.value || undefined }),
							})
						}
					>
						<option value="">All subjects</option>
						{subjects.data?.map((s) => (
							<option key={s.id} value={s.id}>
								{s.name}
							</option>
						))}
					</Select>
				</Field>
				<Field label="Status">
					<Select
						value={search.status ?? ""}
						onChange={(e) =>
							navigate({
								search: (s) => ({ ...s, status: e.target.value || undefined }),
							})
						}
					>
						<option value="">Any status</option>
						{statuses.map((s) => (
							<option key={s.value} value={s.value}>
								{s.label}
							</option>
						))}
					</Select>
				</Field>
			</div>

			{pages.isPending && <Spinner />}
			{pages.error && <ErrorNote error={pages.error} />}
			{pages.data?.length === 0 && (
				<Empty title="No pages here">Upload pages from the Upload tab.</Empty>
			)}
			<div className="flex flex-col gap-2.5">
				{pages.data?.map((p) => (
					<PageRow key={p.id} page={p} />
				))}
			</div>

			<SystemCheck />
		</>
	);
}

/** The API's own health report, for when something looks stuck. */
function SystemCheck() {
	const health = useQuery({
		queryKey: ["health"],
		queryFn: () => unwrap(api.GET("/health")),
	});
	if (!health.data) return null;
	const rows = [
		["Database", health.data.database],
		["Photo storage", health.data.storage],
		["Claude", health.data.claude],
		["OpenAI", health.data.openai],
	];
	return (
		<Card fill="cream" className="mt-4">
			<Label>System</Label>
			<ul className="mt-2 flex flex-col gap-1 text-sm font-bold">
				{rows.map(([name, status]) => (
					<li key={name} className="flex gap-2">
						<span>{status.startsWith("ok") ? "✓" : "✗"}</span>
						<span className="w-28 shrink-0 font-extrabold">{name}</span>
						<span className="min-w-0 truncate text-muted">{status}</span>
					</li>
				))}
			</ul>
		</Card>
	);
}
