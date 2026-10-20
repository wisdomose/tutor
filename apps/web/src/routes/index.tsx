import { useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute, Link } from "@tanstack/react-router";
import {
	BookOpen,
	Camera,
	FileText,
	Images,
	PenLine,
	Plus,
	Upload,
	X,
} from "lucide-react";
import {
	type ChangeEvent,
	type ReactNode,
	useEffect,
	useRef,
	useState,
} from "react";
import { PageRow } from "#/components/PageRow";
import { QueuePanel } from "#/components/QueuePanel";
import { describeSource, SourceForm } from "#/components/SourceForm";
import {
	Button,
	Card,
	cx,
	ErrorNote,
	PageTitle,
	Select,
} from "#/components/ui";
import { api, unwrap } from "#/lib/api";
import { countPdfPages, renderPdfPages } from "#/lib/pdf";
import { shrinkPhoto, uploadPdf, uploadPhotos } from "#/lib/photos";

export const Route = createFileRoute("/")({ component: UploadPage });

const MAX_PHOTOS = 20;
/** Rendered PDF pages go up this many at a time. */
const PDF_BATCH = 10;
const SOURCE_KEY = "tutor.source";

type Picked = { id: string; file: File; preview: string };
type PickedPdf = { file: File; pages: number | null };

function UploadPage() {
	const qc = useQueryClient();
	const sources = useQuery({
		queryKey: ["sources"],
		queryFn: () => unwrap(api.GET("/sources")),
	});
	const exams = useQuery({
		queryKey: ["exams"],
		queryFn: () => unwrap(api.GET("/exams")),
	});
	const recent = useQuery({
		queryKey: ["pages", { limit: 8 }],
		queryFn: () =>
			unwrap(api.GET("/pages", { params: { query: { limit: 8 } } })),
		refetchInterval: 5_000,
	});

	const [sourceId, setSourceId] = useState("");
	const [adding, setAdding] = useState(false);
	// Showing the source picker again after one was chosen.
	const [changing, setChanging] = useState(false);
	const [picked, setPicked] = useState<Picked[]>([]);
	const [pdf, setPdf] = useState<PickedPdf | null>(null);
	const [progress, setProgress] = useState<string | null>(null);
	const [error, setError] = useState<unknown>(null);
	const [done, setDone] = useState<number | null>(null);
	const cameraInput = useRef<HTMLInputElement>(null);
	const galleryInput = useRef<HTMLInputElement>(null);
	const pdfInput = useRef<HTMLInputElement>(null);

	// Remember the last source on this device.
	useEffect(() => {
		try {
			const saved = localStorage.getItem(SOURCE_KEY);
			if (saved) setSourceId(saved);
		} catch {}
	}, []);
	const chooseSource = (id: string) => {
		setSourceId(id);
		try {
			localStorage.setItem(SOURCE_KEY, id);
		} catch {}
	};

	const addPdf = (e: ChangeEvent<HTMLInputElement>) => {
		const file = e.target.files?.[0];
		e.target.value = "";
		if (!file) return;
		setDone(null);
		setError(null);
		for (const p of picked) URL.revokeObjectURL(p.preview);
		setPicked([]);
		setPdf({ file, pages: null });
		countPdfPages(file).then(
			(pages) => setPdf((cur) => (cur?.file === file ? { file, pages } : cur)),
			(err) => {
				setPdf(null);
				setError(new Error(`Couldn't open that PDF: ${err?.message ?? err}`));
			},
		);
	};

	const addFiles = (e: ChangeEvent<HTMLInputElement>) => {
		const files = Array.from(e.target.files ?? []);
		e.target.value = "";
		setDone(null);
		setError(null);
		setPdf(null);
		setPicked((prev) => {
			const room = MAX_PHOTOS - prev.length;
			if (files.length > room)
				setError(
					new Error(
						`Up to ${MAX_PHOTOS} photos per upload — the rest were left out.`,
					),
				);
			return [
				...prev,
				...files.slice(0, Math.max(0, room)).map((file) => ({
					id: crypto.randomUUID(),
					file,
					preview: URL.createObjectURL(file),
				})),
			];
		});
	};
	const remove = (id: string) =>
		setPicked((prev) => {
			const gone = prev.find((p) => p.id === id);
			if (gone) URL.revokeObjectURL(gone.preview);
			return prev.filter((p) => p.id !== id);
		});

	const source = sources.data?.find((s) => s.id === sourceId);
	const examName = (id?: string | null) => {
		const e = exams.data?.find((x) => x.id === id);
		return e && `${e.body} ${e.code}`;
	};
	const pageCount = pdf ? (pdf.pages ?? 0) : picked.length;
	const canUpload = !!source && pageCount > 0 && progress === null;

	const uploadPicked = async (sourceId: string) => {
		const photos: Blob[] = [];
		for (const [i, p] of picked.entries()) {
			setProgress(`Preparing photo ${i + 1} of ${picked.length}…`);
			photos.push(await shrinkPhoto(p.file));
		}
		await uploadPhotos(sourceId, photos, (f) =>
			setProgress(`Uploading… ${Math.round(f * 100)}%`),
		);
		for (const p of picked) URL.revokeObjectURL(p.preview);
		setPicked([]);
		return picked.length;
	};

	const uploadPickedPdf = async (sourceId: string, file: File) => {
		const uploadId = await uploadPdf(sourceId, file, (f) =>
			setProgress(`Uploading the PDF… ${Math.round(f * 100)}%`),
		);
		let batch: Blob[] = [];
		let sent = 0;
		const send = async (total: number) => {
			await uploadPhotos(
				sourceId,
				batch,
				(f) =>
					setProgress(
						`Sending pages ${sent + 1}–${sent + batch.length} of ${total}… ${Math.round(f * 100)}%`,
					),
				uploadId,
			);
			sent += batch.length;
			batch = [];
			qc.invalidateQueries({ queryKey: ["pages"] });
		};
		for await (const { page, pages, jpeg } of renderPdfPages(file)) {
			setProgress(`Preparing page ${page} of ${pages}…`);
			batch.push(jpeg);
			if (batch.length === PDF_BATCH || page === pages) await send(pages);
		}
		setPdf(null);
		return sent;
	};

	const upload = async () => {
		if (!canUpload) return;
		setError(null);
		try {
			setDone(
				pdf
					? await uploadPickedPdf(source.id, pdf.file)
					: await uploadPicked(source.id),
			);
			qc.invalidateQueries({ queryKey: ["pages"] });
			qc.invalidateQueries({ queryKey: ["queue"] });
			qc.invalidateQueries({ queryKey: ["sources"] });
		} catch (e) {
			setError(e);
		} finally {
			setProgress(null);
		}
	};

	return (
		<>
			<PageTitle>Question converter</PageTitle>

			<Card fill="lavender">
				<div className="flex flex-col gap-4">
					<input
						ref={cameraInput}
						type="file"
						accept="image/*"
						capture="environment"
						hidden
						onChange={addFiles}
					/>
					<input
						ref={galleryInput}
						type="file"
						accept="image/*"
						multiple
						hidden
						onChange={addFiles}
					/>
					<input
						ref={pdfInput}
						type="file"
						accept="application/pdf"
						hidden
						onChange={addPdf}
					/>

					<Step n={1} label="Book or paper">
						{adding ? (
							<SourceForm
								onCreated={(s) => {
									chooseSource(s.id);
									setAdding(false);
									setChanging(false);
								}}
								onCancel={() => setAdding(false)}
							/>
						) : source && !changing ? (
							<div className="flex items-start gap-3 rounded-[14px] border-[2.5px] border-ink bg-paper px-3 py-2.5">
								<div className="mt-0.5 flex size-9 shrink-0 items-center justify-center rounded-[10px] border-2 border-ink bg-lavender-soft">
									{source.kind === "paper" ? (
										<FileText
											aria-hidden="true"
											className="size-5"
											strokeWidth={2.5}
										/>
									) : (
										<BookOpen
											aria-hidden="true"
											className="size-5"
											strokeWidth={2.5}
										/>
									)}
								</div>
								<div className="min-w-0 flex-1">
									<div className="font-display text-lg leading-tight font-extrabold">
										{source.title}
									</div>
									<div className="mt-0.5 text-xs font-bold text-muted">
										{source.subject} ·{" "}
										{describeSource(source, examName(source.exam_id))}
										<br />
										{source.page_count === 0
											? "No pages yet"
											: `${source.page_count} page${source.page_count === 1 ? "" : "s"} so far`}
									</div>
								</div>
								<button
									type="button"
									disabled={progress !== null}
									onClick={() => setChanging(true)}
									className="flex items-center gap-1 text-xs font-extrabold underline"
								>
									<PenLine
										aria-hidden="true"
										className="size-3.5"
										strokeWidth={2.75}
									/>
									Change
								</button>
							</div>
						) : (
							<div className="flex flex-col gap-2.5">
								<Select
									value={source ? sourceId : ""}
									aria-label="Book or paper"
									onChange={(e) => {
										chooseSource(e.target.value);
										setChanging(false);
									}}
								>
									<option value="" disabled>
										{sources.data?.length === 0
											? "None yet — add one below"
											: "Choose one…"}
									</option>
									{groupBySubject(sources.data ?? []).map(([subject, list]) => (
										<optgroup key={subject} label={subject}>
											{list.map((s) => (
												<option key={s.id} value={s.id}>
													{s.title}
												</option>
											))}
										</optgroup>
									))}
								</Select>
								<div className="flex gap-4">
									<button
										type="button"
										onClick={() => setAdding(true)}
										className="flex items-center gap-1 text-sm font-extrabold underline"
									>
										<Plus
											aria-hidden="true"
											className="size-4"
											strokeWidth={3}
										/>
										Add a new book or paper
									</button>
									{source && (
										<button
											type="button"
											onClick={() => setChanging(false)}
											className="text-sm font-extrabold text-muted underline"
										>
											Cancel
										</button>
									)}
								</div>
							</div>
						)}
					</Step>

					{!adding && (
						<Step
							n={2}
							label={
								pdf
									? "PDF"
									: picked.length > 0
										? `Pages · ${picked.length} of ${MAX_PHOTOS}`
										: "Pages"
							}
							off={!source}
						>
							{!source ? (
								<p className="text-sm font-bold">
									Choose a book or paper first.
								</p>
							) : pdf ? (
								<div className="flex items-center gap-3 rounded-[14px] border-[2.5px] border-ink bg-paper px-3 py-2.5">
									<FileText
										aria-hidden="true"
										className="size-6 shrink-0"
										strokeWidth={2.5}
									/>
									<div className="min-w-0 flex-1">
										<div className="truncate text-sm font-extrabold">
											{pdf.file.name}
										</div>
										<div className="text-xs font-bold text-muted">
											{pdf.pages === null
												? "Counting pages…"
												: `${pdf.pages} page${pdf.pages === 1 ? "" : "s"}, sent in order`}
										</div>
									</div>
									<RemoveButton
										label="Remove PDF"
										disabled={progress !== null}
										onClick={() => setPdf(null)}
									/>
								</div>
							) : picked.length > 0 ? (
								<div className="flex flex-col gap-2">
									<div className="grid grid-cols-4 gap-3 sm:grid-cols-5">
										{picked.map((p, i) => (
											<div key={p.id} className="relative">
												<img
													src={p.preview}
													alt={`Page ${i + 1}`}
													className="aspect-[3/4] w-full rounded-[10px] border-2 border-ink object-cover"
												/>
												<span className="absolute -top-2 -left-2 rounded-[7px] bg-ink px-1.5 font-display text-xs font-extrabold text-lime">
													{i + 1}
												</span>
												<span className="absolute -top-2 -right-2">
													<RemoveButton
														label={`Remove photo ${i + 1}`}
														disabled={progress !== null}
														onClick={() => remove(p.id)}
													/>
												</span>
											</div>
										))}
										{picked.length < MAX_PHOTOS && (
											<button
												type="button"
												aria-label="Take another photo"
												disabled={progress !== null}
												onClick={() => cameraInput.current?.click()}
												className="flex aspect-[3/4] items-center justify-center rounded-[10px] border-2 border-dashed border-ink"
											>
												<Camera
													aria-hidden="true"
													className="size-6"
													strokeWidth={2.5}
												/>
											</button>
										)}
									</div>
									{picked.length < MAX_PHOTOS && (
										<button
											type="button"
											disabled={progress !== null}
											onClick={() => galleryInput.current?.click()}
											className="flex items-center gap-1 self-start text-xs font-extrabold underline"
										>
											<Images
												aria-hidden="true"
												className="size-3.5"
												strokeWidth={2.75}
											/>
											Add from your photos
										</button>
									)}
								</div>
							) : (
								<div className="flex flex-col gap-2 rounded-[14px] border-[2.5px] border-dashed border-ink bg-paper/30 p-2.5">
									<Button
										icon={Camera}
										onClick={() => cameraInput.current?.click()}
									>
										Take photo
									</Button>
									<div className="flex gap-2">
										<Button
											size="sm"
											fill="paper"
											className="flex-1"
											icon={Images}
											onClick={() => galleryInput.current?.click()}
										>
											Photos
										</Button>
										<Button
											size="sm"
											fill="paper"
											className="flex-1"
											icon={FileText}
											onClick={() => pdfInput.current?.click()}
										>
											PDF
										</Button>
									</div>
									<p className="text-center text-xs font-bold">
										In book order · up to {MAX_PHOTOS} photos
									</p>
								</div>
							)}
						</Step>
					)}

					{error !== null && <ErrorNote error={error} />}

					{!adding && (canUpload || progress !== null) && (
						<Button icon={Upload} disabled={!canUpload} onClick={upload}>
							{progress ??
								`Upload ${pageCount} page${pageCount === 1 ? "" : "s"}`}
						</Button>
					)}
				</div>
			</Card>

			{done !== null && (
				<Card fill="lime">
					<div className="font-display text-xl font-extrabold">
						{done} page{done === 1 ? "" : "s"} uploaded
					</div>
					<div className="text-sm font-bold">
						They're in the queue. Flagged questions will appear in Review.
					</div>
				</Card>
			)}

			<QueuePanel />

			<div className="mt-2 flex items-baseline justify-between">
				<h2 className="font-display text-[21px] font-extrabold">
					Latest uploads
				</h2>
				<Link to="/uploads" className="text-sm font-extrabold underline">
					See all
				</Link>
			</div>
			{recent.error && <ErrorNote error={recent.error} />}
			{recent.data?.length === 0 && (
				<p className="text-sm font-bold text-muted">Nothing uploaded yet.</p>
			)}
			<div className="flex flex-col gap-2.5">
				{recent.data?.map((p) => (
					<PageRow key={p.id} page={p} />
				))}
			</div>
		</>
	);
}

function groupBySubject<T extends { subject: string }>(list: T[]) {
	const groups = new Map<string, T[]>();
	for (const s of list)
		groups.set(s.subject, [...(groups.get(s.subject) ?? []), s]);
	return [...groups].sort(([a], [b]) => a.localeCompare(b));
}

/** A numbered step of the upload card; faded until it can be used. */
function Step({
	n,
	label,
	off,
	children,
}: {
	n: number;
	label: string;
	off?: boolean;
	children: ReactNode;
}) {
	return (
		<section className={cx("flex flex-col gap-2.5", off && "opacity-50")}>
			<div className="flex items-center gap-2 text-xs font-extrabold tracking-[1.2px] uppercase">
				<span
					className={cx(
						"flex size-5 items-center justify-center rounded-[6px] font-display text-xs tracking-normal",
						off ? "border-2 border-ink" : "bg-ink text-lime",
					)}
				>
					{n}
				</span>
				{label}
			</div>
			{children}
		</section>
	);
}

function RemoveButton({
	label,
	disabled,
	onClick,
}: {
	label: string;
	disabled?: boolean;
	onClick: () => void;
}) {
	return (
		<button
			type="button"
			onClick={onClick}
			disabled={disabled}
			aria-label={label}
			className="flex size-6 items-center justify-center rounded-full border-2 border-ink bg-paper"
		>
			<X aria-hidden="true" className="size-3.5" strokeWidth={3.5} />
		</button>
	);
}
