import { useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute, Link } from "@tanstack/react-router";
import { type ChangeEvent, useEffect, useRef, useState } from "react";
import { PageRow } from "#/components/PageRow";
import { QueuePanel } from "#/components/QueuePanel";
import {
	Button,
	Card,
	ErrorNote,
	Field,
	Label,
	PageTitle,
	Select,
} from "#/components/ui";
import { api, unwrap } from "#/lib/api";
import { shrinkPhoto, uploadPhotos } from "#/lib/photos";

export const Route = createFileRoute("/")({ component: UploadPage });

const MAX_PHOTOS = 20;
const SUBJECT_KEY = "tutor.subject";

type Picked = { id: string; file: File; preview: string };

function UploadPage() {
	const qc = useQueryClient();
	const subjects = useQuery({
		queryKey: ["subjects"],
		queryFn: () => unwrap(api.GET("/subjects")),
	});
	const recent = useQuery({
		queryKey: ["pages", { limit: 8 }],
		queryFn: () =>
			unwrap(api.GET("/pages", { params: { query: { limit: 8 } } })),
		refetchInterval: 5_000,
	});

	const [subjectId, setSubjectId] = useState("");
	const [picked, setPicked] = useState<Picked[]>([]);
	const [progress, setProgress] = useState<string | null>(null);
	const [error, setError] = useState<unknown>(null);
	const [done, setDone] = useState<number | null>(null);
	const cameraInput = useRef<HTMLInputElement>(null);
	const galleryInput = useRef<HTMLInputElement>(null);

	// Remember the last subject on this device.
	useEffect(() => {
		try {
			const saved = localStorage.getItem(SUBJECT_KEY);
			if (saved) setSubjectId(saved);
		} catch {}
	}, []);
	const chooseSubject = (id: string) => {
		setSubjectId(id);
		try {
			localStorage.setItem(SUBJECT_KEY, id);
		} catch {}
	};

	const addFiles = (e: ChangeEvent<HTMLInputElement>) => {
		const files = Array.from(e.target.files ?? []);
		e.target.value = "";
		setDone(null);
		setError(null);
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

	const subjectName = subjects.data?.find((s) => s.id === subjectId)?.name;
	const canUpload = !!subjectName && picked.length > 0 && progress === null;

	const upload = async () => {
		if (!canUpload) return;
		setError(null);
		try {
			const photos: Blob[] = [];
			for (const [i, p] of picked.entries()) {
				setProgress(`Preparing photo ${i + 1} of ${picked.length}…`);
				photos.push(await shrinkPhoto(p.file));
			}
			await uploadPhotos(subjectId, photos, (f) =>
				setProgress(`Uploading… ${Math.round(f * 100)}%`),
			);
			for (const p of picked) URL.revokeObjectURL(p.preview);
			setDone(picked.length);
			setPicked([]);
			qc.invalidateQueries({ queryKey: ["pages"] });
			qc.invalidateQueries({ queryKey: ["queue"] });
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
					<Field label="Subject">
						<Select
							value={subjectId}
							onChange={(e) => chooseSubject(e.target.value)}
						>
							<option value="" disabled>
								Choose a subject…
							</option>
							{subjects.data?.map((s) => (
								<option key={s.id} value={s.id}>
									{s.name}
								</option>
							))}
						</Select>
					</Field>

					<div>
						<Label>
							Pages ({picked.length}/{MAX_PHOTOS})
						</Label>
						{picked.length > 0 && (
							<div className="mt-2 grid grid-cols-4 gap-2 sm:grid-cols-5">
								{picked.map((p, i) => (
									<div key={p.id} className="relative">
										<img
											src={p.preview}
											alt={`Page ${i + 1}`}
											className="aspect-[3/4] w-full rounded-[11px] border-[2.5px] border-ink object-cover"
										/>
										<button
											type="button"
											onClick={() => remove(p.id)}
											aria-label={`Remove photo ${i + 1}`}
											className="absolute -top-2 -right-2 flex size-7 items-center justify-center rounded-full border-2 border-ink bg-paper font-extrabold"
										>
											×
										</button>
									</div>
								))}
							</div>
						)}
						<div className="mt-3 flex gap-3">
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
							<Button
								fill="paper"
								className="flex-1"
								onClick={() => cameraInput.current?.click()}
							>
								Take photo
							</Button>
							<Button
								fill="paper"
								className="flex-1"
								onClick={() => galleryInput.current?.click()}
							>
								Choose photos
							</Button>
						</div>
					</div>

					{error !== null && <ErrorNote error={error} />}

					<Button disabled={!canUpload} onClick={upload}>
						{progress ??
							(picked.length === 0
								? "Add pages to upload"
								: !subjectName
									? "Choose a subject first"
									: `Upload ${picked.length} page${picked.length === 1 ? "" : "s"} · ${subjectName}`)}
					</Button>
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
