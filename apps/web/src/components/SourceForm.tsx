import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { NewSource, Source } from "@tutor/api-client";
import {
	BookOpen,
	CalendarDays,
	Check,
	ChevronLeft,
	ChevronRight,
	FileText,
	GraduationCap,
	ListTree,
	type LucideIcon,
	PenLine,
} from "lucide-react";
import { type ReactNode, useState } from "react";
import {
	Button,
	cx,
	ErrorNote,
	Field,
	Select,
	TextInput,
} from "#/components/ui";
import { api, unwrap } from "#/lib/api";

export const sittingLabels: Record<string, string> = {
	may_june: "May/June",
	june_july: "June/July",
	nov_dec: "Nov/Dec",
};

/** "Book · by topic · WAEC WASSCE" — what the reader will assume about pages that don't say. */
export function describeSource(s: Source, examName?: string) {
	const parts =
		s.kind === "paper"
			? [
					examName ?? "Exam paper",
					s.year,
					s.paper && `Paper ${s.paper}`,
					s.sitting && sittingLabels[s.sitting],
				]
			: [
					s.layout === "by_topic" ? "Book by topic" : "Book by year",
					examName,
					s.publisher,
					s.edition,
				];
	return parts.filter(Boolean).join(" · ");
}

type StepKey = "kind" | "subject" | "layout" | "details" | "check";

/** Adds a book or exam paper step by step: kind, subject, arrangement (books), details, check. */
export function SourceForm({ onCreated }: { onCreated: (s: Source) => void }) {
	const qc = useQueryClient();
	const subjects = useQuery({
		queryKey: ["subjects"],
		queryFn: () => unwrap(api.GET("/subjects")),
	});
	const exams = useQuery({
		queryKey: ["exams"],
		queryFn: () => unwrap(api.GET("/exams")),
	});

	const [at, setAt] = useState<StepKey>("kind");
	const [kind, setKind] = useState<"book" | "paper" | null>(null);
	const [subjectId, setSubjectId] = useState("");
	const [layout, setLayout] = useState<"by_topic" | "by_year" | null>(null);
	const [title, setTitle] = useState("");
	// A paper's title follows its details until it's typed over.
	const [titleTyped, setTitleTyped] = useState(false);
	const [examId, setExamId] = useState("");
	const [year, setYear] = useState("");
	const [paper, setPaper] = useState("");
	const [sitting, setSitting] = useState("");
	const [publisher, setPublisher] = useState("");
	const [edition, setEdition] = useState("");

	const subject = subjects.data?.find((s) => s.id === subjectId);
	const exam = exams.data?.find((e) => e.id === examId);
	const examLabel = exam && `${exam.body} ${exam.code}`;
	const levelExams = exams.data?.filter(
		(e) => !subject || e.level === subject.level,
	);
	const isPaper = kind === "paper";
	const autoTitle = [examLabel, subject?.name, year, paper && `Paper ${paper}`]
		.filter(Boolean)
		.join(" ");
	const finalTitle = isPaper && !titleTyped ? autoTitle : title;

	const create = useMutation({
		mutationFn: (body: NewSource) => unwrap(api.POST("/sources", { body })),
		onSuccess: (s) => {
			qc.invalidateQueries({ queryKey: ["sources"] });
			onCreated(s);
		},
	});

	// A paper is one exam sitting, so it has no arrangement step.
	const steps: StepKey[] = isPaper
		? ["kind", "subject", "details", "check"]
		: ["kind", "subject", "layout", "details", "check"];
	const index = steps.indexOf(at);
	const done: Record<StepKey, boolean> = {
		kind: kind !== null,
		subject: subjectId !== "",
		layout: layout !== null,
		details:
			finalTitle.trim() !== "" &&
			(!isPaper || (examId !== "" && /^\d{4}$/.test(year))),
		check: true,
	};
	const next = () => setAt(steps[index + 1]);

	const submit = () =>
		create.mutate({
			kind: kind ?? "book",
			layout: isPaper ? "single_paper" : (layout ?? "by_topic"),
			title: finalTitle,
			subject_id: subjectId,
			exam_id: examId || null,
			year: isPaper ? Number(year) : null,
			paper: isPaper ? paper || null : null,
			sitting: isPaper ? sitting || null : null,
			publisher: isPaper ? null : publisher || null,
			edition: isPaper ? null : edition || null,
		});

	return (
		<div className="flex flex-col gap-4">
			<div className="flex items-center gap-3">
				<div className="flex flex-1 gap-1.5" aria-hidden="true">
					{steps.map((key, n) => (
						<span
							key={key}
							className={cx(
								"h-2 flex-1 rounded-full border-2 border-ink",
								n <= index ? "bg-ink" : "bg-paper",
							)}
						/>
					))}
				</div>
			</div>

			{at === "kind" && (
				<div className="grid grid-cols-2 gap-3">
					<KindTile
						icon={BookOpen}
						label="Book"
						on={kind === "book"}
						onClick={() => {
							setKind("book");
							setAt("subject");
						}}
					/>
					<KindTile
						icon={FileText}
						label="Exam paper"
						on={kind === "paper"}
						onClick={() => {
							setKind("paper");
							setAt("subject");
						}}
					/>
				</div>
			)}

			{at === "subject" && (
				<Field label="Subject">
					<Select
						value={subjectId}
						onChange={(e) => {
							setSubjectId(e.target.value);
							setExamId("");
						}}
					>
						<option value="" disabled>
							Choose…
						</option>
						{(["sss", "jss"] as const).map((lvl) => {
							const list = subjects.data?.filter((s) => s.level === lvl);
							return list?.length ? (
								<optgroup key={lvl} label={lvl.toUpperCase()}>
									{list.map((s) => (
										<option key={s.id} value={s.id}>
											{s.name}
										</option>
									))}
								</optgroup>
							) : null;
						})}
					</Select>
				</Field>
			)}

			{at === "layout" && (
				<div className="grid grid-cols-2 gap-3">
					<KindTile
						icon={ListTree}
						label="By topic"
						on={layout === "by_topic"}
						onClick={() => {
							setLayout("by_topic");
							setAt("details");
						}}
					/>
					<KindTile
						icon={CalendarDays}
						label="By year"
						on={layout === "by_year"}
						onClick={() => {
							setLayout("by_year");
							setAt("details");
						}}
					/>
				</div>
			)}

			{at === "details" && (
				<div className="flex flex-col gap-4">
					{!isPaper && (
						<Field label="Title">
							<TextInput
								value={title}
								autoFocus
								onChange={(e) => setTitle(e.target.value)}
							/>
						</Field>
					)}
					<Field label="Exam">
						<Select value={examId} onChange={(e) => setExamId(e.target.value)}>
							<option value="">{isPaper ? "Choose…" : "Any"}</option>
							{levelExams?.map((e) => (
								<option key={e.id} value={e.id}>
									{e.body} {e.code}
								</option>
							))}
						</Select>
					</Field>
					{isPaper ? (
						<>
							<div className="grid grid-cols-[5fr_4fr_7fr] gap-3">
								<Field label="Year">
									<TextInput
										inputMode="numeric"
										maxLength={4}
										value={year}
										onChange={(e) => setYear(e.target.value.trim())}
									/>
								</Field>
								<Field label="Paper">
									<TextInput
										inputMode="numeric"
										value={paper}
										onChange={(e) => setPaper(e.target.value)}
									/>
								</Field>
								<Field label="Sitting">
									<Select
										value={sitting}
										onChange={(e) => setSitting(e.target.value)}
									>
										<option value="">—</option>
										{Object.entries(sittingLabels).map(([v, l]) => (
											<option key={v} value={v}>
												{l}
											</option>
										))}
									</Select>
								</Field>
							</div>
							<Field label="Title">
								<TextInput
									value={finalTitle}
									onChange={(e) => {
										setTitleTyped(true);
										setTitle(e.target.value);
									}}
								/>
							</Field>
						</>
					) : (
						<div className="grid grid-cols-2 gap-3">
							<Field label="Publisher">
								<TextInput
									value={publisher}
									placeholder="Optional"
									onChange={(e) => setPublisher(e.target.value)}
								/>
							</Field>
							<Field label="Edition">
								<TextInput
									value={edition}
									placeholder="Optional"
									onChange={(e) => setEdition(e.target.value)}
								/>
							</Field>
						</div>
					)}
				</div>
			)}

			{at === "check" && (
				<div className="flex flex-col divide-y-2 divide-ink rounded-[14px] border-[2.5px] border-ink bg-paper">
					<SummaryRow
						icon={isPaper ? FileText : BookOpen}
						onEdit={() => setAt("kind")}
					>
						{isPaper ? "Exam paper" : "Book"}
					</SummaryRow>
					<SummaryRow icon={GraduationCap} onEdit={() => setAt("subject")}>
						{subject?.name} · {subject?.level.toUpperCase()}
					</SummaryRow>
					{!isPaper && (
						<SummaryRow
							icon={layout === "by_year" ? CalendarDays : ListTree}
							onEdit={() => setAt("layout")}
						>
							{layout === "by_year" ? "By year" : "By topic"}
						</SummaryRow>
					)}
					<SummaryRow icon={PenLine} onEdit={() => setAt("details")}>
						<span className="font-display text-base">{finalTitle}</span>
						<span className="block text-xs text-muted">
							{(isPaper
								? [
										examLabel,
										year,
										paper && `Paper ${paper}`,
										sitting && sittingLabels[sitting],
									]
								: [examLabel, publisher, edition]
							)
								.filter(Boolean)
								.join(" · ")}
						</span>
					</SummaryRow>
				</div>
			)}

			{create.error && <ErrorNote error={create.error} />}

			{at !== "kind" && (
				<div className="flex gap-3">
					<Button
						fill="paper"
						icon={ChevronLeft}
						className="flex-1"
						onClick={() => setAt(steps[index - 1])}
					>
						Back
					</Button>
					{at !== "check" ? (
						<Button
							icon={ChevronRight}
							iconEnd
							className="flex-[1.4]"
							disabled={!done[at]}
							onClick={next}
						>
							Next
						</Button>
					) : (
						<Button
							icon={Check}
							className="flex-[1.4]"
							disabled={create.isPending}
							onClick={submit}
						>
							{create.isPending ? "Adding…" : "Add"}
						</Button>
					)}
				</div>
			)}
		</div>
	);
}

function KindTile({
	icon: Icon,
	label,
	on,
	onClick,
}: {
	icon: LucideIcon;
	label: string;
	on: boolean;
	onClick: () => void;
}) {
	return (
		<button
			type="button"
			onClick={onClick}
			className="group rounded-[16px] bg-ink"
		>
			<span
				className={cx(
					"flex -translate-y-1 flex-col items-center gap-2 rounded-[16px] border-[2.5px] border-ink px-3 py-5 font-display text-lg font-extrabold transition-transform duration-75 group-active:translate-y-0",
					on ? "bg-lime" : "bg-paper",
				)}
			>
				<Icon aria-hidden="true" className="size-8" strokeWidth={2.25} />
				{label}
			</span>
		</button>
	);
}

function SummaryRow({
	icon: Icon,
	onEdit,
	children,
}: {
	icon: LucideIcon;
	onEdit: () => void;
	children: ReactNode;
}) {
	return (
		<button
			type="button"
			onClick={onEdit}
			className="flex items-center gap-3 px-3 py-2.5 text-left text-sm font-extrabold"
		>
			<Icon aria-hidden="true" className="size-5 shrink-0" strokeWidth={2.5} />
			<span className="min-w-0 flex-1">{children}</span>
			<PenLine
				aria-hidden="true"
				className="size-4 shrink-0 text-muted"
				strokeWidth={2.5}
			/>
		</button>
	);
}
