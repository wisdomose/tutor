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
	X,
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

/** Adds a book or exam paper in four steps: kind, subject, details, check. */
export function SourceForm({
	onCreated,
	onCancel,
}: {
	onCreated: (s: Source) => void;
	onCancel: () => void;
}) {
	const qc = useQueryClient();
	const subjects = useQuery({
		queryKey: ["subjects"],
		queryFn: () => unwrap(api.GET("/subjects")),
	});
	const exams = useQuery({
		queryKey: ["exams"],
		queryFn: () => unwrap(api.GET("/exams")),
	});

	const [step, setStep] = useState(1);
	const [kind, setKind] = useState<"book" | "paper" | null>(null);
	const [subjectId, setSubjectId] = useState("");
	const [layout, setLayout] = useState<"by_topic" | "by_year">("by_topic");
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

	const stepDone = [
		true,
		subjectId !== "",
		finalTitle.trim() !== "" &&
			(!isPaper || (examId !== "" && /^\d{4}$/.test(year))),
		true,
	];

	const submit = () =>
		create.mutate({
			kind: kind ?? "book",
			layout: isPaper ? "single_paper" : layout,
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
				<div className="flex flex-1 gap-1.5" aria-label={`Step ${step} of 4`}>
					{[1, 2, 3, 4].map((n) => (
						<span
							key={n}
							className={cx(
								"h-2 flex-1 rounded-full border-2 border-ink",
								n <= step ? "bg-ink" : "bg-paper",
							)}
						/>
					))}
				</div>
				<span className="font-display text-sm font-extrabold">{step}/4</span>
				<button
					type="button"
					aria-label="Cancel"
					onClick={onCancel}
					className="flex size-8 items-center justify-center rounded-[10px] border-2 border-ink bg-paper"
				>
					<X aria-hidden="true" className="size-4" strokeWidth={3} />
				</button>
			</div>

			{step === 1 && (
				<div className="grid grid-cols-2 gap-3">
					<KindTile
						icon={BookOpen}
						label="Book"
						on={kind === "book"}
						onClick={() => {
							setKind("book");
							setStep(2);
						}}
					/>
					<KindTile
						icon={FileText}
						label="Exam paper"
						on={kind === "paper"}
						onClick={() => {
							setKind("paper");
							setStep(2);
						}}
					/>
				</div>
			)}

			{step === 2 && (
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

			{step === 3 && (
				<div className="flex flex-col gap-4">
					{!isPaper && (
						<>
							<Field label="Title">
								<TextInput
									value={title}
									autoFocus
									onChange={(e) => setTitle(e.target.value)}
								/>
							</Field>
							<Toggle
								value={layout}
								onChange={setLayout}
								options={[
									{ value: "by_topic", label: "By topic", icon: ListTree },
									{ value: "by_year", label: "By year", icon: CalendarDays },
								]}
							/>
						</>
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

			{step === 4 && (
				<div className="flex flex-col divide-y-2 divide-ink rounded-[14px] border-[2.5px] border-ink bg-paper">
					<SummaryRow
						icon={isPaper ? FileText : BookOpen}
						onEdit={() => setStep(1)}
					>
						{isPaper ? "Exam paper" : "Book"}
					</SummaryRow>
					<SummaryRow icon={GraduationCap} onEdit={() => setStep(2)}>
						{subject?.name} · {subject?.level.toUpperCase()}
					</SummaryRow>
					<SummaryRow icon={PenLine} onEdit={() => setStep(3)}>
						<span className="font-display text-base">{finalTitle}</span>
						<span className="block text-xs text-muted">
							{(isPaper
								? [
										examLabel,
										year,
										paper && `Paper ${paper}`,
										sitting && sittingLabels[sitting],
									]
								: [
										layout === "by_topic" ? "By topic" : "By year",
										examLabel,
										publisher,
										edition,
									]
							)
								.filter(Boolean)
								.join(" · ")}
						</span>
					</SummaryRow>
				</div>
			)}

			{create.error && <ErrorNote error={create.error} />}

			{step > 1 && (
				<div className="flex gap-3">
					<Button
						fill="paper"
						icon={ChevronLeft}
						className="flex-1"
						onClick={() => setStep(step - 1)}
					>
						Back
					</Button>
					{step < 4 ? (
						<Button
							icon={ChevronRight}
							iconEnd
							className="flex-[1.4]"
							disabled={!stepDone[step - 1]}
							onClick={() => setStep(step + 1)}
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

function Toggle<T extends string>({
	value,
	onChange,
	options,
}: {
	value: T;
	onChange: (v: T) => void;
	options: { value: T; label: string; icon: LucideIcon }[];
}) {
	return (
		<div className="flex gap-2">
			{options.map(({ value: v, label, icon: Icon }) => (
				<button
					key={v}
					type="button"
					onClick={() => onChange(v)}
					aria-pressed={value === v}
					className={cx(
						"flex flex-1 items-center justify-center gap-2 rounded-[13px] border-[2.5px] border-ink px-3 py-2 font-display font-extrabold",
						value === v ? "bg-ink text-lime" : "bg-paper",
					)}
				>
					<Icon aria-hidden="true" className="size-4" strokeWidth={2.75} />
					{label}
				</button>
			))}
		</div>
	);
}
