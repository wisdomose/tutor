import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { PagePhoto } from "./PagePhoto";
import { Link, useNavigate } from "@tanstack/react-router";
import type { Diagram, QuestionDetail } from "@tutor/api-client";
import { useState } from "react";
import { api, apiPath, unwrap } from "#/lib/api";
import { MathText } from "./MathText";
import { Reasons } from "./Reasons";
import {
	Button,
	Card,
	Chip,
	cx,
	ErrorNote,
	Field,
	Label,
	Location,
	Spinner,
	TextArea,
	TextInput,
} from "./ui";

/**
 * Review / edit one question. In "review" mode, "Looks right" saves, marks it
 * checked and moves to the next flagged question.
 */
export function QuestionEditor({
	id,
	mode,
}: {
	id: string;
	mode: "review" | "bank";
}) {
	const question = useQuery({
		queryKey: ["question", id],
		queryFn: () =>
			unwrap(api.GET("/questions/{id}", { params: { path: { id } } })),
		// Keep polling while a diagram is being (re)drawn.
		refetchInterval: (q) =>
			q.state.data && hasPendingDiagram(q.state.data) ? 4_000 : false,
	});
	if (question.isPending) return <Spinner />;
	if (question.error) return <ErrorNote error={question.error} />;
	// Keyed so the form resets when moving to the next question.
	return <Editor key={question.data.id} q={question.data} mode={mode} />;
}

function hasPendingDiagram(q: QuestionDetail) {
	const diagrams = [q.diagram, ...q.options.map((o) => o.diagram)];
	return diagrams.some(
		(d) => d && (d.status === "queued" || d.status === "drawing"),
	);
}

function Editor({ q, mode }: { q: QuestionDetail; mode: "review" | "bank" }) {
	const qc = useQueryClient();
	const navigate = useNavigate();
	const [body, setBody] = useState(q.body);
	const [topic, setTopic] = useState(q.topic ?? "");
	const [explanation, setExplanation] = useState(q.explanation ?? "");
	const [options, setOptions] = useState(() =>
		q.options.map((o) => ({ id: o.id, body: o.body ?? "" })),
	);
	const [correctId, setCorrectId] = useState(
		q.options.find((o) => o.is_correct)?.id ?? null,
	);
	const [savedNote, setSavedNote] = useState<string | null>(null);

	const save = useMutation({
		mutationFn: (resolve: boolean) =>
			unwrap(
				api.PUT("/questions/{id}", {
					params: { path: { id: q.id } },
					body: {
						body,
						topic: topic || null,
						explanation: explanation || null,
						options: options.map((o) => ({
							id: o.id,
							body: o.body.trim() === "" ? null : o.body,
						})),
						correct_option_id: correctId,
						resolve,
					},
				}),
			),
		onSuccess: (updated, resolve) => {
			qc.setQueryData(["question", q.id], updated);
			qc.invalidateQueries({ queryKey: ["review"] });
			qc.invalidateQueries({ queryKey: ["questions"] });
			qc.invalidateQueries({ queryKey: ["page", q.page_id] });
			if (resolve && mode === "review") {
				if (updated.next_flagged_id)
					navigate({
						to: "/review/$questionId",
						params: { questionId: updated.next_flagged_id },
					});
				else navigate({ to: "/review" });
			} else {
				setSavedNote(resolve ? "Saved and marked as checked" : "Saved");
			}
		},
	});
	const remove = useMutation({
		mutationFn: () =>
			unwrap(api.DELETE("/questions/{id}", { params: { path: { id: q.id } } })),
		onSuccess: () => {
			qc.invalidateQueries({ queryKey: ["review"] });
			qc.invalidateQueries({ queryKey: ["questions"] });
			qc.invalidateQueries({ queryKey: ["page", q.page_id] });
			if (mode === "review" && q.next_flagged_id)
				navigate({
					to: "/review/$questionId",
					params: { questionId: q.next_flagged_id },
				});
			else navigate({ to: mode === "review" ? "/review" : "/questions" });
		},
	});

	const correctLabel = q.options.find((o) => o.id === correctId)?.label;
	const solLabel = q.options.find((o) => o.is_correct)?.label;
	const showSecondOpinion =
		q.second_answer && q.second_answer.toUpperCase() !== solLabel;
	const resolved = !!q.resolved_at;

	return (
		<>
			<div className="flex flex-wrap items-center gap-2">
				<Location subject={q.subject} page={q.page_number} number={q.number} />
				{q.needs_check && !resolved && <Reasons reasons={q.check_reasons} />}
				{resolved && <Chip fill="lime">Checked</Chip>}
			</div>

			<div className="grid gap-4 md:grid-cols-[minmax(0,2fr)_minmax(0,3fr)]">
				{/* Original photo, to compare with the book */}
				<div className="flex flex-col gap-2">
					<PagePhoto pageId={q.page_id} url={q.photo_url} alt="Original page" />
					<Link
						to="/uploads/$pageId"
						params={{ pageId: q.page_id }}
						className="text-sm font-extrabold underline"
					>
						All questions on this page
					</Link>
				</div>

				<div className="flex flex-col gap-4">
					{q.instruction && (
						<Card fill="lavender-soft">
							<Label>Shared instruction</Label>
							<p className="mt-1 text-[15px] font-bold">
								<MathText text={q.instruction} />
							</p>
						</Card>
					)}
					{q.diagram && <DiagramPanel diagram={q.diagram} questionId={q.id} />}

					<Field label="Question">
						<TextArea
							value={body}
							onChange={(e) => setBody(e.target.value)}
							rows={3}
						/>
					</Field>
					{body.includes("$") && (
						<div className="-mt-2 rounded-[13px] border-2 border-dashed border-ink px-3 py-2 text-[15px] font-bold">
							<MathText text={body} />
						</div>
					)}

					<div className="flex flex-col gap-2">
						<Label>Options — tap a letter to mark the correct answer</Label>
						{q.options.map((o, i) => {
							const isCorrect = o.id === correctId;
							return (
								<div
									key={o.id}
									className={cx(
										"flex items-start gap-3 rounded-[18px] border-ink p-2.5",
										isCorrect
											? "border-[3px] bg-lime"
											: "border-[2.5px] bg-paper",
									)}
								>
									<button
										type="button"
										onClick={() => setCorrectId(o.id)}
										aria-label={`Mark ${o.label} as correct`}
										aria-pressed={isCorrect}
										className={cx(
											"mt-0.5 flex size-[38px] shrink-0 items-center justify-center rounded-[12px] font-display text-[17px] font-extrabold",
											isCorrect
												? "bg-ink text-lime"
												: "border-[2.5px] border-ink bg-lavender-soft",
										)}
									>
										{o.label}
									</button>
									<div className="flex min-w-0 flex-1 flex-col gap-2">
										<TextInput
											value={options[i].body}
											placeholder={o.diagram ? "(picture only)" : ""}
											onChange={(e) =>
												setOptions((prev) =>
													prev.map((p, j) =>
														j === i ? { ...p, body: e.target.value } : p,
													),
												)
											}
										/>
										{o.diagram && (
											<DiagramPanel
												diagram={o.diagram}
												questionId={q.id}
												compact
											/>
										)}
									</div>
									{isCorrect && (
										<span className="mt-2.5 hidden sm:inline">
											<Chip fill="paper">Correct</Chip>
										</span>
									)}
								</div>
							);
						})}
						{!correctId && (
							<p className="text-sm font-extrabold">No answer selected yet.</p>
						)}
					</div>

					{showSecondOpinion && (
						<Card fill="lavender">
							<Label>Second opinion</Label>
							<p className="mt-1 text-[15px] font-bold">
								gpt-6.1-sol picked <strong>{solLabel ?? "nothing"}</strong>;
								gpt-6-luna picked <strong>{q.second_answer}</strong>.
							</p>
							{q.second_explanation && (
								<p className="mt-2 rounded-[13px] border-2 border-ink bg-cream p-2.5 text-sm font-bold">
									Luna: <MathText text={q.second_explanation} />
								</p>
							)}
						</Card>
					)}

					<Field
						label={`Explanation${correctLabel ? ` (for ${correctLabel})` : ""}`}
					>
						<TextArea
							value={explanation}
							onChange={(e) => setExplanation(e.target.value)}
							rows={3}
						/>
					</Field>
					<Field label="Topic">
						<TextInput
							value={topic}
							onChange={(e) => setTopic(e.target.value)}
							placeholder="e.g. Levels of Organization"
						/>
					</Field>

					{(save.error || remove.error) && (
						<ErrorNote error={save.error ?? remove.error} />
					)}
					{savedNote && !save.isPending && (
						<p className="text-sm font-extrabold">{savedNote} ✓</p>
					)}

					<div className="flex gap-3">
						<Button
							fill="paper"
							className="flex-1"
							disabled={save.isPending}
							onClick={() => save.mutate(false)}
						>
							Save
						</Button>
						<Button
							className="flex-[1.6]"
							disabled={save.isPending}
							onClick={() => save.mutate(true)}
						>
							{save.isPending
								? "Saving…"
								: mode === "review"
									? "Looks right → next"
									: resolved
										? "Save"
										: "Mark checked"}
						</Button>
					</div>
					<button
						type="button"
						className="self-start text-sm font-extrabold text-muted underline"
						disabled={remove.isPending}
						onClick={() => {
							if (
								confirm(
									"Delete this question? Use this for things that are not real questions.",
								)
							)
								remove.mutate();
						}}
					>
						Delete question
					</button>
				</div>
			</div>
		</>
	);
}

/** The AI-drawn diagram with Accept / Redraw-with-note controls. */
function DiagramPanel({
	diagram,
	questionId,
	compact,
}: {
	diagram: Diagram;
	questionId: string;
	compact?: boolean;
}) {
	const qc = useQueryClient();
	const [note, setNote] = useState("");
	const [asking, setAsking] = useState(false);
	const refresh = () =>
		qc.invalidateQueries({ queryKey: ["question", questionId] });
	const accept = useMutation({
		mutationFn: () =>
			unwrap(
				api.POST("/diagrams/{id}/accept", {
					params: { path: { id: diagram.id } },
				}),
			),
		onSuccess: refresh,
	});
	const redraw = useMutation({
		mutationFn: () =>
			unwrap(
				api.POST("/diagrams/{id}/redraw", {
					params: { path: { id: diagram.id } },
					body: { note: note.trim() || null },
				}),
			),
		onSuccess: () => {
			setAsking(false);
			setNote("");
			refresh();
		},
	});
	const pending = diagram.status === "queued" || diagram.status === "drawing";

	return (
		<Card fill="paper" className={compact ? "p-2.5" : undefined}>
			{!compact && (
				<div className="mb-2 flex items-center gap-2">
					<Label>Diagram (redrawn by AI — compare with the photo)</Label>
					{diagram.reviewed_at && <Chip fill="lime">Accepted</Chip>}
				</div>
			)}
			{pending && (
				<Spinner
					label={
						diagram.status === "queued" ? "Waiting to be drawn…" : "Drawing…"
					}
				/>
			)}
			{diagram.status === "failed" && (
				<p className="text-sm font-bold">
					Drawing failed: {diagram.error ?? "unknown error"}
				</p>
			)}
			{diagram.svg_url && !pending && (
				<img
					src={apiPath(diagram.svg_url)}
					alt="Redrawn diagram"
					className={cx(
						"w-full rounded-[11px] bg-white",
						compact ? "max-h-40 object-contain" : "max-h-[50vh] object-contain",
					)}
				/>
			)}
			{diagram.redraw_note && pending && (
				<p className="mt-1 text-xs font-bold text-muted">
					Fixing: {diagram.redraw_note}
				</p>
			)}
			{!pending && (
				<div className="mt-3 flex flex-col gap-2">
					{asking ? (
						<>
							<TextArea
								rows={2}
								value={note}
								autoFocus
								placeholder="What's wrong? e.g. label III should point to the roots"
								onChange={(e) => setNote(e.target.value)}
							/>
							<div className="flex gap-2">
								<Button size="sm" fill="paper" onClick={() => setAsking(false)}>
									Cancel
								</Button>
								<Button
									size="sm"
									disabled={redraw.isPending}
									onClick={() => redraw.mutate()}
								>
									Redraw
								</Button>
							</div>
						</>
					) : (
						<div className="flex gap-2">
							{diagram.status === "done" && !diagram.reviewed_at && (
								<Button
									size="sm"
									disabled={accept.isPending}
									onClick={() => accept.mutate()}
								>
									Accept
								</Button>
							)}
							<Button size="sm" fill="paper" onClick={() => setAsking(true)}>
								Redraw…
							</Button>
						</div>
					)}
					{(accept.error || redraw.error) && (
						<ErrorNote error={accept.error ?? redraw.error} />
					)}
				</div>
			)}
		</Card>
	);
}
