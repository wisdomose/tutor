import { useQuery } from "@tanstack/react-query";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { QuestionRow } from "#/components/QuestionRow";
import {
	Button,
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

type Search = { subject?: string };

export const Route = createFileRoute("/review/")({
	validateSearch: (s: Record<string, unknown>): Search => ({
		subject: typeof s.subject === "string" ? s.subject : undefined,
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
	const first = review.data?.items[0];

	return (
		<>
			<PageTitle>Review</PageTitle>
			<Card fill={review.data?.total ? "lavender" : "lime"}>
				<div className="flex items-center gap-3">
					<div className="flex-1">
						<Label>To check</Label>
						<div className="font-display text-[30px] leading-none font-extrabold">
							{review.data?.total ?? "…"}
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
						navigate({ search: { subject: e.target.value || undefined } })
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

			{review.isPending && <Spinner />}
			{review.error && <ErrorNote error={review.error} />}
			{review.data?.total === 0 && (
				<Empty title="All checked">Nothing needs checking right now.</Empty>
			)}
			<div className="flex flex-col gap-2.5">
				{review.data?.items.map((q) => (
					<QuestionRow key={q.id} q={q} to="/review/$questionId" />
				))}
			</div>
		</>
	);
}
