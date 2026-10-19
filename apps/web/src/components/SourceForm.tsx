import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { NewSource, Source } from "@tutor/api-client";
import { type FormEvent, useState } from "react";
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

/** Creates a book or exam paper. Books keep only an exam default; a paper is one exam sitting. */
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

	const [kind, setKind] = useState<"book" | "paper">("book");
	const [layout, setLayout] = useState<"by_topic" | "by_year">("by_topic");
	const [title, setTitle] = useState("");
	const [subjectId, setSubjectId] = useState("");
	const [examId, setExamId] = useState("");
	const [year, setYear] = useState("");
	const [paper, setPaper] = useState("");
	const [sitting, setSitting] = useState("");
	const [publisher, setPublisher] = useState("");
	const [edition, setEdition] = useState("");

	const level = subjects.data?.find((s) => s.id === subjectId)?.level;
	const levelExams = exams.data?.filter((e) => !level || e.level === level);

	const create = useMutation({
		mutationFn: (body: NewSource) => unwrap(api.POST("/sources", { body })),
		onSuccess: (s) => {
			qc.invalidateQueries({ queryKey: ["sources"] });
			onCreated(s);
		},
	});

	const isPaper = kind === "paper";
	const ready =
		title.trim() !== "" &&
		subjectId !== "" &&
		(!isPaper || (examId !== "" && /^\d{4}$/.test(year)));

	const submit = (e: FormEvent) => {
		e.preventDefault();
		if (!ready) return;
		create.mutate({
			kind,
			layout: isPaper ? "single_paper" : layout,
			title,
			subject_id: subjectId,
			exam_id: examId || null,
			year: isPaper ? Number(year) : null,
			paper: isPaper ? paper || null : null,
			sitting: isPaper ? sitting || null : null,
			publisher: isPaper ? null : publisher || null,
			edition: isPaper ? null : edition || null,
		});
	};

	return (
		<form onSubmit={submit} className="flex flex-col gap-4">
			<Toggle
				value={kind}
				onChange={setKind}
				options={[
					{ value: "book", label: "Book" },
					{ value: "paper", label: "Exam paper" },
				]}
			/>

			<Field label="Title">
				<TextInput
					value={title}
					onChange={(e) => setTitle(e.target.value)}
					placeholder={isPaper ? "WAEC Biology 1993" : "Prep50 Biology"}
				/>
			</Field>

			<Field label="Subject">
				<Select
					value={subjectId}
					onChange={(e) => {
						setSubjectId(e.target.value);
						setExamId("");
					}}
				>
					<option value="" disabled>
						Choose a subject…
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

			{!isPaper && (
				<Field label="Arranged">
					<Toggle
						value={layout}
						onChange={setLayout}
						options={[
							{ value: "by_topic", label: "By topic" },
							{ value: "by_year", label: "By year" },
						]}
					/>
				</Field>
			)}

			<Field label={isPaper ? "Exam" : "Exam (if the whole book is one)"}>
				<Select value={examId} onChange={(e) => setExamId(e.target.value)}>
					<option value="">
						{isPaper ? "Choose an exam…" : "More than one, or not stated"}
					</option>
					{levelExams?.map((e) => (
						<option key={e.id} value={e.id}>
							{e.body} {e.code}
						</option>
					))}
				</Select>
			</Field>

			{isPaper ? (
				<div className="grid grid-cols-3 gap-3">
					<Field label="Year">
						<TextInput
							inputMode="numeric"
							value={year}
							onChange={(e) => setYear(e.target.value.trim())}
							placeholder="1993"
						/>
					</Field>
					<Field label="Paper">
						<TextInput
							value={paper}
							onChange={(e) => setPaper(e.target.value)}
							placeholder="2"
						/>
					</Field>
					<Field label="Sitting">
						<Select
							value={sitting}
							onChange={(e) => setSitting(e.target.value)}
						>
							<option value="">Not stated</option>
							{Object.entries(sittingLabels).map(([v, l]) => (
								<option key={v} value={v}>
									{l}
								</option>
							))}
						</Select>
					</Field>
				</div>
			) : (
				<div className="grid grid-cols-2 gap-3">
					<Field label="Publisher">
						<TextInput
							value={publisher}
							onChange={(e) => setPublisher(e.target.value)}
							placeholder="Optional"
						/>
					</Field>
					<Field label="Edition">
						<TextInput
							value={edition}
							onChange={(e) => setEdition(e.target.value)}
							placeholder="Optional"
						/>
					</Field>
				</div>
			)}

			{create.error && <ErrorNote error={create.error} />}

			<div className="flex gap-3">
				<Button fill="paper" className="flex-1" onClick={onCancel}>
					Cancel
				</Button>
				<Button
					type="submit"
					className="flex-1"
					disabled={!ready || create.isPending}
				>
					{create.isPending ? "Saving…" : "Add source"}
				</Button>
			</div>
		</form>
	);
}

function Toggle<T extends string>({
	value,
	onChange,
	options,
}: {
	value: T;
	onChange: (v: T) => void;
	options: { value: T; label: string }[];
}) {
	return (
		<div className="flex gap-2">
			{options.map((o) => (
				<button
					key={o.value}
					type="button"
					onClick={() => onChange(o.value)}
					aria-pressed={value === o.value}
					className={cx(
						"flex-1 rounded-[13px] border-[2.5px] border-ink px-3 py-2 font-display font-extrabold",
						value === o.value ? "bg-ink text-lime" : "bg-paper",
					)}
				>
					{o.label}
				</button>
			))}
		</div>
	);
}
