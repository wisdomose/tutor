import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import type { Diagram, QuestionDetail, QuestionGroup } from "@tutor/api-client";
import { useState } from "react";
import { api, apiPath, unwrap } from "#/lib/api";
import { MathText } from "./MathText";
import { PagePhoto } from "./PagePhoto";
import { Reasons } from "./Reasons";
import { sittingLabels } from "./SourceForm";
import { useToast } from "./Toast";
import {
	Button,
	Card,
	Chip,
	cx,
	ErrorNote,
	Field,
	Label,
	Location,
	Select,
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
	const diagrams = [
		q.diagram,
		...q.options.map((o) => o.diagram),
		...q.parts.map((p) => p.diagram),
	];
	return diagrams.some(
		(d) => d && (d.status === "queued" || d.status === "drawing"),
	);
}

type PartState = {
	id: string;
	body: string;
	closing_text: string;
	marks: string;
};

const numberOrNull = (s: string) => {
	const n = Number(s.trim());
	return s.trim() === "" || Number.isNaN(n) ? null : n;
};

function Editor({ q, mode }: { q: QuestionDetail; mode: "review" | "bank" }) {
	const qc = useQueryClient();
	const navigate = useNavigate();
	const theory = q.kind === "theory";
	const [body, setBody] = useState(q.body);
	const [topicId, setTopicId] = useState(q.topic_id ?? "");
	const topics = useQuery({
		queryKey: ["topics", q.subject_id],
		queryFn: () =>
			unwrap(
				api.GET("/topics", { params: { query: { subject_id: q.subject_id } } }),
			),
	});
	const [explanation, setExplanation] = useState(q.explanation ?? "");
	const [options, setOptions] = useState(() =>
		q.options.map((o) => ({ id: o.id, body: o.body ?? "" })),
	);
	const [correctId, setCorrectId] = useState(
		q.options.find((o) => o.is_correct)?.id ?? null,
	);
	const [parts, setParts] = useState<PartState[]>(() =>
		q.parts.map((p) => ({
			id: p.id,
			body: p.body ?? "",
			closing_text: p.closing_text ?? "",
			marks: p.marks?.toString() ?? "",
		})),
	);
	const [setting, setSetting] = useState<SettingState>(() => ({
		exam_id: q.exam_id ?? "",
		year: q.year?.toString() ?? "",
		paper: q.paper ?? "",
		sitting: q.sitting ?? "",
		original_number: q.original_number ?? "",
		marks: q.marks?.toString() ?? "",
	}));
	const toast = useToast();

	const invalidate = () => {
		qc.invalidateQueries({ queryKey: ["review"] });
		qc.invalidateQueries({ queryKey: ["questions"] });
		qc.invalidateQueries({ queryKey: ["page"] });
	};
	const goNext = () => {
		if (mode === "review" && q.next_flagged_id)
			navigate({
				to: "/review/$questionId",
				params: { questionId: q.next_flagged_id },
			});
		else navigate({ to: mode === "review" ? "/review" : "/questions" });
	};

	const save = useMutation({
		mutationFn: (resolve: boolean) =>
			unwrap(
				api.PUT("/questions/{id}", {
					params: { path: { id: q.id } },
					body: {
						body,
						topic_id: topicId || null,
						explanation: explanation || null,
						options: options.map((o) => ({
							id: o.id,
							body: o.body.trim() === "" ? null : o.body,
						})),
						correct_option_id: correctId,
						parts: parts.map((p) => ({
							id: p.id,
							body: p.body || null,
							closing_text: p.closing_text || null,
							marks: numberOrNull(p.marks),
						})),
						exam_id: setting.exam_id || null,
						year: numberOrNull(setting.year),
						paper: setting.paper || null,
						sitting: setting.sitting || null,
						original_number: setting.original_number || null,
						marks: numberOrNull(setting.marks),
						resolve,
					},
				}),
			),
		onSuccess: (updated, resolve) => {
			qc.setQueryData(["question", q.id], updated);
			invalidate();
			if (resolve && mode === "review") {
				if (updated.next_flagged_id)
					navigate({
						to: "/review/$questionId",
						params: { questionId: updated.next_flagged_id },
					});
				else navigate({ to: "/review" });
			} else {
				toast(resolve ? "Saved and checked" : "Saved");
			}
		},
	});
	const remove = useMutation({
		mutationFn: () =>
			unwrap(api.DELETE("/questions/{id}", { params: { path: { id: q.id } } })),
		onSuccess: () => {
			toast("Question deleted");
			invalidate();
			goNext();
		},
	});
	const merge = useMutation({
		mutationFn: (into: string) =>
			unwrap(
				api.POST("/questions/{id}/merge", {
					params: { path: { id: q.id } },
					body: { into },
				}),
			),
		onSuccess: (kept) => {
			toast("Merged");
			qc.setQueryData(["question", kept.id], kept);
			qc.removeQueries({ queryKey: ["question", q.id] });
			invalidate();
			if (mode === "review") goNext();
			else
				navigate({
					to: "/questions/$questionId",
					params: { questionId: kept.id },
				});
		},
	});

	const correctLabel = q.options.find((o) => o.id === correctId)?.label;
	const solLabel = q.options.find((o) => o.is_correct)?.label;
	const showSecondOpinion =
		!theory && q.second_answer && q.second_answer.toUpperCase() !== solLabel;
	const resolved = !!q.resolved_at;
	const error = save.error ?? remove.error ?? merge.error;

	return (
		<>
			<div className="flex flex-wrap items-center gap-2">
				<Location
					subject={q.source ?? q.subject}
					page={q.page_number}
					number={q.number}
				/>
				{theory && <Chip fill="paper">Theory</Chip>}
				{q.needs_check && !resolved && <Reasons reasons={q.check_reasons} />}
				{resolved && <Chip fill="lime">Checked</Chip>}
			</div>

			<div className="grid grid-cols-[minmax(0,1fr)] gap-4 md:grid-cols-[minmax(0,2fr)_minmax(0,3fr)]">
				<PhotoPanel q={q} />

				<div className="flex flex-col gap-4">
					{q.related.length > 0 && (
						<RelatedPanel
							q={q}
							mode={mode}
							merging={merge.isPending}
							onMerge={(id) => merge.mutate(id)}
						/>
					)}
					<SettingFields
						subjectId={q.subject_id}
						theory={theory}
						value={setting}
						onChange={setSetting}
					/>
					{q.group && (
						<GroupPanel group={q.group} questionId={q.id} mode={mode} />
					)}
					{q.diagram && (
						<DiagramPanel
							diagram={q.diagram}
							questionId={q.id}
							sharedBy={q.group?.members.length}
						/>
					)}

					<Field label={theory ? "Lead text (before the parts)" : "Question"}>
						<TextArea
							value={body}
							onChange={(e) => setBody(e.target.value)}
							rows={theory && body === "" ? 1 : 3}
							placeholder={theory ? "None — the question starts at (a)" : ""}
						/>
					</Field>
					<MathPreview text={body} />

					{theory ? (
						<PartsEditor
							q={q}
							parts={parts}
							onChange={(i, patch) =>
								setParts((prev) =>
									prev.map((p, j) => (j === i ? { ...p, ...patch } : p)),
								)
							}
						/>
					) : (
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
											<MathPreview text={options[i].body} />
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
								<p className="text-sm font-extrabold">
									No answer selected yet.
								</p>
							)}
						</div>
					)}

					{showSecondOpinion && (
						<Card fill="lavender">
							<Label>Second opinion</Label>
							<p className="mt-1 text-[15px] font-bold">
								gpt-6.1-sol picked <strong>{solLabel ?? "nothing"}</strong>,
								gpt-6-luna picked <strong>{q.second_answer}</strong>.
							</p>
							{q.second_explanation && (
								<p className="mt-2 rounded-[13px] border-2 border-ink bg-cream p-2.5 text-sm font-bold">
									Luna: <MathText text={q.second_explanation} />
								</p>
							)}
						</Card>
					)}

					{!theory && (
						<Field
							label={`Explanation${correctLabel ? ` (for ${correctLabel})` : ""}`}
						>
							<TextArea
								value={explanation}
								onChange={(e) => setExplanation(e.target.value)}
								rows={3}
							/>
						</Field>
					)}
					{!theory && <MathPreview text={explanation} />}
					<Field label="Topic">
						<Select
							value={topicId}
							onChange={(e) => setTopicId(e.target.value)}
							disabled={!topics.data?.length}
						>
							<option value="">
								{topics.data?.length
									? "No topic"
									: "No syllabus topics for this subject yet"}
							</option>
							{topics.data?.map((t) => (
								<option key={t.id} value={t.id}>
									{t.name}
								</option>
							))}
						</Select>
					</Field>

					{error && <ErrorNote error={error} />}

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

/**
 * The page photo to compare against. A question printed in several places has a
 * photo for each, so a reviewer can compare the copies.
 */
function PhotoPanel({ q }: { q: QuestionDetail }) {
	const [shown, setShown] = useState(0);
	const a = q.appearances[shown] ?? q.appearances[0];
	if (!a) return <div />;
	return (
		<div className="flex flex-col gap-2">
			{q.appearances.length > 1 && (
				<div className="flex flex-col gap-1.5">
					<Label>Printed in {q.appearances.length} places</Label>
					<div className="flex flex-wrap gap-1.5">
						{q.appearances.map((x, i) => (
							<button
								key={`${x.page_id}-${x.number}`}
								type="button"
								onClick={() => setShown(i)}
								aria-pressed={i === shown}
								className={cx(
									"rounded-[10px] border-2 border-ink px-2 py-0.5 text-left text-xs font-extrabold",
									i === shown ? "bg-ink text-lime" : "bg-paper",
								)}
							>
								{x.source} · p{x.page_number ?? "?"} · Q{x.number}
							</button>
						))}
					</div>
				</div>
			)}
			<PagePhoto
				key={a.page_id}
				pageId={a.page_id}
				url={a.photo_url}
				alt={`${a.source}, page ${a.page_number ?? "?"}`}
			/>
			<Link
				to="/uploads/$pageId"
				params={{ pageId: a.page_id }}
				className="text-sm font-extrabold underline"
			>
				All questions on this page
			</Link>
		</div>
	);
}

type SettingState = {
	exam_id: string;
	year: string;
	paper: string;
	sitting: string;
	original_number: string;
	marks: string;
};

/** Where the question was set. Matching across books relies on these. */
function SettingFields({
	subjectId,
	theory,
	value,
	onChange,
}: {
	subjectId: string;
	theory: boolean;
	value: SettingState;
	onChange: (v: SettingState) => void;
}) {
	const subjects = useQuery({
		queryKey: ["subjects"],
		queryFn: () => unwrap(api.GET("/subjects")),
	});
	const exams = useQuery({
		queryKey: ["exams"],
		queryFn: () => unwrap(api.GET("/exams")),
	});
	const level = subjects.data?.find((s) => s.id === subjectId)?.level;
	const set = (patch: Partial<SettingState>) =>
		onChange({ ...value, ...patch });
	return (
		<Card>
			<Label>Where it was set</Label>
			<div className="mt-3 grid grid-cols-2 gap-3">
				<div className="col-span-2">
					<Field label="Exam">
						<Select
							value={value.exam_id}
							onChange={(e) => set({ exam_id: e.target.value })}
						>
							<option value="">Not known</option>
							{exams.data
								?.filter((e) => !level || e.level === level)
								.map((e) => (
									<option key={e.id} value={e.id}>
										{e.body} {e.code}
									</option>
								))}
						</Select>
					</Field>
				</div>
				<Field label="Year">
					<TextInput
						inputMode="numeric"
						value={value.year}
						placeholder="—"
						onChange={(e) => set({ year: e.target.value })}
					/>
				</Field>
				<Field label="Exam's Q no.">
					<TextInput
						value={value.original_number}
						placeholder="—"
						onChange={(e) => set({ original_number: e.target.value })}
					/>
				</Field>
				<Field label="Paper">
					<TextInput
						value={value.paper}
						placeholder="—"
						onChange={(e) => set({ paper: e.target.value })}
					/>
				</Field>
				<Field label="Sitting">
					<Select
						value={value.sitting}
						onChange={(e) => set({ sitting: e.target.value })}
					>
						<option value="">Not known</option>
						{Object.entries(sittingLabels).map(([v, l]) => (
							<option key={v} value={v}>
								{l}
							</option>
						))}
					</Select>
				</Field>
				{theory && (
					<Field label="Marks">
						<TextInput
							inputMode="decimal"
							value={value.marks}
							placeholder="—"
							onChange={(e) => set({ marks: e.target.value })}
						/>
					</Field>
				)}
			</div>
		</Card>
	);
}

/** A theory question's parts, indented as printed: (a), (a)(i)… */
function PartsEditor({
	q,
	parts,
	onChange,
}: {
	q: QuestionDetail;
	parts: PartState[];
	onChange: (i: number, patch: Partial<PartState>) => void;
}) {
	const [closing, setClosing] = useState<Set<string>>(
		() => new Set(q.parts.filter((p) => p.closing_text).map((p) => p.id)),
	);
	const parents = new Set(q.parts.map((p) => p.parent_id).filter(Boolean));
	if (q.parts.length === 0)
		return (
			<p className="text-sm font-extrabold">
				No parts — the whole question is in the text above.
			</p>
		);
	return (
		<div className="flex flex-col gap-2">
			<Label>Parts</Label>
			{q.parts.map((p, i) => (
				<div
					key={p.id}
					style={{ marginLeft: `${p.depth * 1.75}rem` }}
					className="flex items-start gap-3 rounded-[18px] border-[2.5px] border-ink bg-paper p-2.5"
				>
					<div className="mt-0.5 flex h-[38px] min-w-[38px] shrink-0 items-center justify-center rounded-[12px] border-[2.5px] border-ink bg-lavender-soft px-1.5 font-display text-[15px] font-extrabold">
						({p.label})
					</div>
					<div className="flex min-w-0 flex-1 flex-col gap-2">
						<TextArea
							rows={2}
							value={parts[i].body}
							placeholder="(only sub-parts)"
							onChange={(e) => onChange(i, { body: e.target.value })}
						/>
						<MathPreview text={parts[i].body} />
						{closing.has(p.id) ? (
							<Field label="After its sub-parts">
								<TextArea
									rows={2}
									value={parts[i].closing_text}
									onChange={(e) =>
										onChange(i, { closing_text: e.target.value })
									}
								/>
							</Field>
						) : (
							parents.has(p.id) && (
								<button
									type="button"
									className="self-start text-xs font-extrabold text-muted underline"
									onClick={() => setClosing((s) => new Set(s).add(p.id))}
								>
									Add a sentence after its sub-parts
								</button>
							)
						)}
						{p.diagram && (
							<DiagramPanel diagram={p.diagram} questionId={q.id} compact />
						)}
					</div>
					<div className="w-[4.5rem] shrink-0">
						<TextInput
							inputMode="decimal"
							aria-label={`Marks for (${p.label})`}
							value={parts[i].marks}
							placeholder="marks"
							onChange={(e) => onChange(i, { marks: e.target.value })}
							className="px-2 text-center"
						/>
					</div>
				</div>
			))}
		</div>
	);
}

/**
 * Other questions with the same exam, year and number. A reviewer either merges this
 * one into one of them or keeps them apart ("Looks right").
 */
function RelatedPanel({
	q,
	mode,
	merging,
	onMerge,
}: {
	q: QuestionDetail;
	mode: "review" | "bank";
	merging: boolean;
	onMerge: (id: string) => void;
}) {
	const to =
		mode === "review" ? "/review/$questionId" : "/questions/$questionId";
	const theory = q.kind === "theory";
	return (
		<Card fill="lavender-soft">
			<Label>Same exam, year and number</Label>
			<p className="mt-1 text-sm font-bold text-muted">
				{theory
					? "Books often split one exam question across topics, so these may be other parts of it. Merge only if it's the same text."
					: "Merge if it's the same question. Looks right keeps them apart."}
			</p>
			<div className="mt-3 flex flex-col gap-2">
				{q.related.map((r) => (
					<div
						key={r.id}
						className="rounded-[16px] border-[2.5px] border-ink bg-paper p-3"
					>
						<div className="flex flex-wrap items-center gap-1.5 text-xs font-extrabold">
							<span>
								{r.source ?? "Our question"} · p{r.page_number ?? "?"} · Q
								{r.number}
							</span>
							{r.sitting && (
								<Chip fill="cream">
									{sittingLabels[r.sitting] ?? r.sitting}
								</Chip>
							)}
							{r.paper && <Chip fill="cream">Paper {r.paper}</Chip>}
							<Chip fill="cream">
								{theory
									? `${r.part_count} part${r.part_count === 1 ? "" : "s"}`
									: `${r.option_count} options`}
							</Chip>
						</div>
						<MathText
							text={r.body || "(starts at its parts)"}
							className="mt-1.5 line-clamp-3 text-[15px] leading-snug font-bold"
						/>
						<div className="mt-2.5 flex gap-2">
							<Link
								to={to}
								params={{ questionId: r.id }}
								className="self-center text-sm font-extrabold underline"
							>
								Open
							</Link>
							<span className="flex-1" />
							<Button
								size="sm"
								fill="paper"
								disabled={merging}
								onClick={() => {
									if (
										confirm(
											"Merge into this one? This question is removed and its pages are listed under that one.",
										)
									)
										onMerge(r.id);
								}}
							>
								Merge into this
							</Button>
						</div>
					</div>
				))}
			</div>
		</Card>
	);
}

/**
 * A group's questions are always shown together to students, who never see the
 * printed numbers. Reviewers see the instruction as printed, can correct the
 * student version, and can jump between the group's questions.
 */
function GroupPanel({
	group,
	questionId,
	mode,
}: {
	group: QuestionGroup;
	questionId: string;
	mode: "review" | "bank";
}) {
	const qc = useQueryClient();
	const toast = useToast();
	const [text, setText] = useState(group.display_instruction ?? "");
	const changed = text.trim() !== (group.display_instruction ?? "");
	const save = useMutation({
		mutationFn: () =>
			unwrap(
				api.PUT("/groups/{id}", {
					params: { path: { id: group.id } },
					body: { display_instruction: text.trim() || null },
				}),
			),
		onSuccess: () => {
			toast("Instruction saved");
			for (const m of group.members)
				qc.invalidateQueries({ queryKey: ["question", m.id] });
		},
	});
	const to =
		mode === "review" ? "/review/$questionId" : "/questions/$questionId";

	return (
		<Card fill="lavender-soft">
			<Label>Group · shown together to students</Label>
			<div className="mt-2 flex flex-wrap gap-1.5">
				{group.members.map((m) => (
					<Link
						key={m.id}
						to={to}
						params={{ questionId: m.id }}
						className={cx(
							"rounded-[10px] border-2 border-ink px-2 py-0.5 text-xs font-extrabold",
							m.id === questionId
								? "bg-ink text-lime"
								: m.needs_check && !m.resolved_at
									? "bg-lavender"
									: "bg-paper",
						)}
					>
						Q{m.number}
						{m.resolved_at ? " ✓" : ""}
					</Link>
				))}
			</div>
			{group.instruction && (
				<div className="mt-3">
					<div className="text-[11px] font-extrabold tracking-wide text-muted uppercase">
						As printed
					</div>
					<p className="text-[15px] font-bold">
						<MathText text={group.instruction} />
					</p>
				</div>
			)}
			<div className="mt-3 flex flex-col gap-2">
				<Field label="For students (no question numbers)">
					<TextArea
						rows={2}
						value={text}
						placeholder="e.g. The diagram below is an illustration of a fungus."
						onChange={(e) => setText(e.target.value)}
					/>
				</Field>
				{changed && (
					<Button
						size="sm"
						className="self-start"
						disabled={save.isPending}
						onClick={() => save.mutate()}
					>
						Save instruction
					</Button>
				)}
				{save.error && <ErrorNote error={save.error} />}
			</div>
		</Card>
	);
}

/** The AI-drawn diagram with Accept / Redraw-with-note controls. */
function DiagramPanel({
	diagram,
	questionId,
	compact,
	sharedBy,
}: {
	diagram: Diagram;
	questionId: string;
	compact?: boolean;
	/** Number of questions in the group using this diagram. */
	sharedBy?: number;
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
					<Label>
						Diagram (redrawn by AI — compare with the photo)
						{sharedBy && sharedBy > 1
							? ` · shared by all ${sharedBy} questions in the group`
							: ""}
					</Label>
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

/** The rendered form of text containing maths, under the field that edits it. */
function MathPreview({ text }: { text: string }) {
	if (!text.includes("$")) return null;
	return (
		<div className="rounded-[10px] border-2 border-dashed border-ink px-3 py-2 text-[15px] font-bold">
			<MathText text={text} />
		</div>
	);
}
