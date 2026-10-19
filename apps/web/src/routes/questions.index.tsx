import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { useEffect, useState } from "react";
import { QuestionRow } from "#/components/QuestionRow";
import {
	Button,
	Empty,
	ErrorNote,
	Field,
	PageTitle,
	Select,
	Spinner,
	TextInput,
} from "#/components/ui";
import { api, unwrap } from "#/lib/api";

const PAGE_SIZE = 50;

type Search = {
	subject?: string;
	topic?: string;
	page?: string;
	q?: string;
	offset?: number;
};

const str = (v: unknown) => (typeof v === "string" && v !== "" ? v : undefined);

export const Route = createFileRoute("/questions/")({
	validateSearch: (s: Record<string, unknown>): Search => ({
		subject: str(s.subject),
		topic: str(s.topic),
		page:
			str(s.page) ?? (typeof s.page === "number" ? String(s.page) : undefined),
		q: str(s.q),
		offset: typeof s.offset === "number" && s.offset > 0 ? s.offset : undefined,
	}),
	component: QuestionsPage,
});

function QuestionsPage() {
	const search = Route.useSearch();
	const navigate = useNavigate({ from: Route.fullPath });
	const set = (patch: Partial<Search>) =>
		navigate({ search: (s) => ({ ...s, offset: undefined, ...patch }) });

	const subjects = useQuery({
		queryKey: ["subjects"],
		queryFn: () => unwrap(api.GET("/subjects")),
	});
	const topics = useQuery({
		queryKey: ["topics", search.subject ?? null],
		queryFn: () =>
			unwrap(
				api.GET("/topics", {
					params: { query: { subject_id: search.subject } },
				}),
			),
	});
	const offset = search.offset ?? 0;
	const questions = useQuery({
		queryKey: ["questions", search],
		queryFn: () =>
			unwrap(
				api.GET("/questions", {
					params: {
						query: {
							subject_id: search.subject,
							topic_id: search.topic,
							page_number: search.page,
							q: search.q,
							limit: PAGE_SIZE,
							offset,
						},
					},
				}),
			),
		placeholderData: keepPreviousData,
	});

	// Search box updates the URL after a short pause in typing.
	const [text, setText] = useState(search.q ?? "");
	useEffect(() => {
		const t = setTimeout(() => {
			if ((search.q ?? "") !== text.trim())
				set({ q: text.trim() || undefined });
		}, 400);
		return () => clearTimeout(t);
	});

	const total = questions.data?.total ?? 0;
	return (
		<>
			<PageTitle>Question bank</PageTitle>
			<div className="grid grid-cols-2 gap-3">
				<Field label="Subject">
					<Select
						value={search.subject ?? ""}
						onChange={(e) =>
							set({ subject: e.target.value || undefined, topic: undefined })
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
				<Field label="Topic">
					<Select
						value={search.topic ?? ""}
						onChange={(e) => set({ topic: e.target.value || undefined })}
					>
						<option value="">All topics</option>
						{topics.data?.map((t) => (
							<option key={t.id} value={t.id}>
								{t.name}
							</option>
						))}
					</Select>
				</Field>
				<Field label="Page">
					<TextInput
						inputMode="numeric"
						value={search.page ?? ""}
						placeholder="Any"
						onChange={(e) => set({ page: e.target.value.trim() || undefined })}
					/>
				</Field>
				<Field label="Search">
					<TextInput
						value={text}
						placeholder="Words in a question"
						onChange={(e) => setText(e.target.value)}
					/>
				</Field>
			</div>

			{questions.isPending && <Spinner />}
			{questions.error && <ErrorNote error={questions.error} />}
			{questions.data && (
				<p className="text-sm font-extrabold text-muted">
					{total} question{total === 1 ? "" : "s"}
					{total > PAGE_SIZE &&
						` · showing ${offset + 1}–${Math.min(offset + PAGE_SIZE, total)}`}
				</p>
			)}
			{questions.data?.total === 0 && <Empty title="No questions match" />}
			<div className="flex flex-col gap-2.5">
				{questions.data?.items.map((q) => (
					<QuestionRow key={q.id} q={q} to="/questions/$questionId" />
				))}
			</div>
			{total > PAGE_SIZE && (
				<div className="flex gap-3">
					<Button
						fill="paper"
						className="flex-1"
						disabled={offset === 0}
						onClick={() =>
							navigate({
								search: (s) => ({
									...s,
									offset: Math.max(0, offset - PAGE_SIZE) || undefined,
								}),
							})
						}
					>
						← Previous
					</Button>
					<Button
						fill="paper"
						className="flex-1"
						disabled={offset + PAGE_SIZE >= total}
						onClick={() =>
							navigate({
								search: (s) => ({ ...s, offset: offset + PAGE_SIZE }),
							})
						}
					>
						Next →
					</Button>
				</div>
			)}
		</>
	);
}
