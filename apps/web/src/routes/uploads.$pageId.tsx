import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { MathText } from "#/components/MathText";
import { PagePhoto } from "#/components/PagePhoto";
import { Reasons } from "#/components/Reasons";
import {
	Button,
	Card,
	Chip,
	ErrorNote,
	Label,
	PageTitle,
	Spinner,
	StatusChip,
} from "#/components/ui";
import { api, unwrap } from "#/lib/api";

export const Route = createFileRoute("/uploads/$pageId")({
	component: PageDetailPage,
});

function PageDetailPage() {
	const { pageId } = Route.useParams();
	const qc = useQueryClient();
	const navigate = useNavigate();
	const detail = useQuery({
		queryKey: ["page", pageId],
		queryFn: () =>
			unwrap(api.GET("/pages/{id}", { params: { path: { id: pageId } } })),
		refetchInterval: (q) =>
			q.state.data &&
			["queued", "processing"].includes(q.state.data.page.status)
				? 4_000
				: false,
	});
	const retry = useMutation({
		mutationFn: () =>
			unwrap(
				api.POST("/pages/{id}/retry", { params: { path: { id: pageId } } }),
			),
		onSuccess: () => {
			qc.invalidateQueries({ queryKey: ["page", pageId] });
			qc.invalidateQueries({ queryKey: ["pages"] });
			qc.invalidateQueries({ queryKey: ["review"] });
		},
	});
	const remove = useMutation({
		mutationFn: () =>
			unwrap(api.DELETE("/pages/{id}", { params: { path: { id: pageId } } })),
		onSuccess: () => {
			qc.invalidateQueries({ queryKey: ["pages"] });
			qc.invalidateQueries({ queryKey: ["review"] });
			navigate({ to: "/uploads" });
		},
	});

	if (detail.isPending) return <Spinner />;
	if (detail.error) return <ErrorNote error={detail.error} />;
	const { page, questions, photo_url, tail_fragment } = detail.data;
	const busy = page.status === "queued" || page.status === "processing";

	return (
		<>
			<PageTitle label={page.source}>Page {page.page_number ?? "?"}</PageTitle>
			<div className="-mt-2 flex flex-wrap items-center gap-2">
				<StatusChip status={page.status} />
				{page.status === "done" && (
					<Chip>
						{page.question_count} question{page.question_count === 1 ? "" : "s"}
					</Chip>
				)}
				{page.flagged_count > 0 && (
					<Chip fill="lavender">{page.flagged_count} to check</Chip>
				)}
			</div>

			{page.error && (
				<Card fill="paper">
					<Label>
						{page.status === "failed"
							? "Failed"
							: `Attempt ${page.attempts} failed — retrying`}
					</Label>
					<p className="mt-1 text-sm font-bold break-words">{page.error}</p>
				</Card>
			)}

			<div className="grid gap-4 md:grid-cols-[minmax(0,2fr)_minmax(0,3fr)]">
				<div className="flex flex-col gap-3">
					<PagePhoto
						pageId={page.id}
						url={photo_url}
						alt={`Book page ${page.page_number ?? ""} as uploaded`}
					/>
					<div className="flex gap-3">
						<Button
							fill="paper"
							size="sm"
							className="flex-1"
							disabled={busy || retry.isPending}
							onClick={() => {
								if (
									confirm(
										"Read this page again from scratch? Its questions and any review changes will be replaced.",
									)
								)
									retry.mutate();
							}}
						>
							Re-run
						</Button>
						<Button
							fill="paper"
							size="sm"
							className="flex-1"
							disabled={page.status === "processing" || remove.isPending}
							onClick={() => {
								if (
									confirm("Delete this page, its photo and all its questions?")
								)
									remove.mutate();
							}}
						>
							Delete
						</Button>
					</div>
					{(retry.error || remove.error) && (
						<ErrorNote error={retry.error ?? remove.error} />
					)}
				</div>

				<div className="flex flex-col gap-2.5">
					{busy && (
						<Spinner
							label={
								page.status === "queued"
									? "Waiting its turn…"
									: "Claude is reading this page…"
							}
						/>
					)}
					{questions.map((q) => (
						<Link
							key={q.id}
							to={
								q.needs_check && !q.resolved_at
									? "/review/$questionId"
									: "/questions/$questionId"
							}
							params={{ questionId: q.id }}
							className="rounded-[16px] border-[2.5px] border-ink bg-paper p-3"
						>
							<div className="mb-1 flex flex-wrap items-center gap-1.5">
								<span className="font-display text-base font-extrabold">
									Q{q.number}
								</span>
								{q.needs_check && !q.resolved_at && (
									<Reasons reasons={q.check_reasons} />
								)}
								{q.resolved_at && <Chip fill="lime">Checked</Chip>}
							</div>
							<MathText
								text={q.body}
								className="text-[15px] leading-snug font-bold"
							/>
						</Link>
					))}
					{tail_fragment && (
						<Card fill="lavender-soft">
							<Label>Continues on the next page</Label>
							<p className="mt-1 text-sm font-bold">
								The last question runs onto page{" "}
								{Number(page.page_number) + 1 || "next"}. It's saved here once
								that page is uploaded.
							</p>
						</Card>
					)}
				</div>
			</div>
		</>
	);
}
