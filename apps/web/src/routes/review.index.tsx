import { useQuery } from "@tanstack/react-query";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { type CheckReason, checkReasonLabels } from "@tutor/api-client";
import { QuestionRow } from "#/components/QuestionRow";
import {
	Button,
	Card,
	cx,
	Empty,
	ErrorNote,
	Field,
	Label,
	PageTitle,
	Select,
	Spinner,
} from "#/components/ui";
import { api, unwrap } from "#/lib/api";

type Search = { subject?: string; reason?: string };

export const Route = createFileRoute("/review/")({
	validateSearch: (s: Record<string, unknown>): Search => ({
		subject: typeof s.subject === "string" ? s.subject : undefined,
		reason: typeof s.reason === "string" ? s.reason : undefined,
	}),
	component: ReviewPage,
});

function ReviewPage() {
	const search = Route.useSearch();
	const navigate = useNavigate({ from: Route.fullPath });
	const subjects = useQuery({
		queryKey: ["subjects"],
		queryFn: () => unwrap(api.GET("/subjects")),
	});
	const review = useQuery({
		queryKey: ["review", search.subject ?? null],
		queryFn: () =>
			unwrap(
				api.GET("/review", {
					params: { query: { subject_id: search.subject } },
				}),
			),
		refetchInterval: 15_000,
	});
	// The whole queue comes back at once, so reasons are counted and filtered here.
	const counts = new Map<string, number>();
	for (const q of review.data?.items ?? [])
		for (const r of q.check_reasons) counts.set(r, (counts.get(r) ?? 0) + 1);
	const items = (review.data?.items ?? []).filter(
		(q) => !search.reason || q.check_reasons.includes(search.reason),
	);
	const first = items[0];

	return (
		<>
			<PageTitle>Review</PageTitle>
			<Card fill={review.data?.total ? "lavender" : "lime"}>
				<div className="flex items-center gap-3">
					<div className="flex-1">
						<Label>To check</Label>
						<div className="font-display text-[30px] leading-none font-extrabold">
							{review.data ? items.length : "…"}
						</div>
					</div>
					{first && (
						<Button
							onClick={() =>
								navigate({
									to: "/review/$questionId",
									params: { questionId: first.id },
								})
							}
						>
							Start →
						</Button>
					)}
				</div>
			</Card>

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

			{counts.size > 0 && (
				<div className="flex flex-wrap gap-1.5">
					{[...counts].map(([r, n]) => {
						const on = search.reason === r;
						return (
							<button
								key={r}
								type="button"
								aria-pressed={on}
								onClick={() =>
									navigate({
										search: (s) => ({ ...s, reason: on ? undefined : r }),
									})
								}
								className={cx(
									"rounded-[10px] border-2 border-ink px-2.5 py-1 text-xs font-extrabold tracking-wide uppercase",
									on ? "bg-ink text-lime" : "bg-paper",
								)}
							>
								{checkReasonLabels[r as CheckReason] ?? r} · {n}
							</button>
						);
					})}
				</div>
			)}

			{review.isPending && <Spinner />}
			{review.error && <ErrorNote error={review.error} />}
			{review.data?.total === 0 && (
				<Empty title="All checked">Nothing needs checking right now.</Empty>
			)}
			<div className="flex flex-col gap-2.5">
				{items.map((q) => (
					<QuestionRow key={q.id} q={q} to="/review/$questionId" />
				))}
			</div>
		</>
	);
}
