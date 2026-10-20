import type { PageSummary } from "@tutor/api-client";

const MAX_EDGE = 2400;
const QUALITY = 0.85;

/**
 * Shrinks a phone photo (often 4000px+, 3–8 MB) to at most 2400px on the long
 * edge as JPEG, keeping text sharp enough for Claude while keeping uploads small.
 * EXIF rotation is applied so sideways shots arrive the way they look.
 */
export async function shrinkPhoto(file: File): Promise<Blob> {
	const bitmap = await createImageBitmap(file, {
		imageOrientation: "from-image",
	});
	const scale = Math.min(1, MAX_EDGE / Math.max(bitmap.width, bitmap.height));
	const canvas = document.createElement("canvas");
	canvas.width = Math.round(bitmap.width * scale);
	canvas.height = Math.round(bitmap.height * scale);
	const ctx = canvas.getContext("2d");
	if (!ctx) throw new Error("This browser cannot resize photos");
	ctx.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
	bitmap.close();
	return new Promise((resolve, reject) =>
		canvas.toBlob(
			(b) => (b ? resolve(b) : reject(new Error("Could not encode photo"))),
			"image/jpeg",
			QUALITY,
		),
	);
}

/** Uploads photos of one source in page order, reporting progress 0–1 (fetch can't report upload progress).
 * Pages rendered from a PDF pass the PDF's `uploadId`. */
export function uploadPhotos(
	sourceId: string,
	photos: Blob[],
	onProgress: (fraction: number) => void,
	uploadId?: string,
): Promise<PageSummary[]> {
	const form = new FormData();
	form.append("source_id", sourceId);
	if (uploadId) form.append("upload_id", uploadId);
	photos.forEach((p, i) => {
		form.append("photos", p, `page-${i + 1}.jpg`);
	});
	return postForm("/api/pages", form, onProgress);
}

/** Above this a PDF isn't sent whole (it may outlast the proxy's time limit); only its pages are. */
const KEEP_PDF_BYTES = 5 * 1024 * 1024;

/** Registers the PDF (keeping the file when small); its pages follow through `uploadPhotos`. */
export async function uploadPdf(
	sourceId: string,
	pdf: File,
	onProgress: (fraction: number) => void,
): Promise<string> {
	const form = new FormData();
	form.append("source_id", sourceId);
	if (pdf.size <= KEEP_PDF_BYTES) form.append("pdf", pdf, pdf.name);
	else form.append("name", pdf.name);
	const body = await postForm<{ upload_id: string }>(
		"/api/uploads/pdf",
		form,
		onProgress,
	);
	return body.upload_id;
}

/** An upload the network dropped (worth retrying), as opposed to one the API refused. */
export class NetworkError extends Error {}

/** Runs `send` up to three times while the failure is the network's, waiting a little longer each time. */
export async function withRetry<T>(send: () => Promise<T>): Promise<T> {
	for (let attempt = 1; ; attempt++) {
		try {
			return await send();
		} catch (e) {
			if (!(e instanceof NetworkError) || attempt === 3) throw e;
			await new Promise((r) => setTimeout(r, attempt * 2000));
		}
	}
}

/** The proxy in front of the API cuts a request after about a minute. */
const REQUEST_TIMEOUT_MS = 90_000;

function postForm<T>(
	url: string,
	form: FormData,
	onProgress: (fraction: number) => void,
): Promise<T> {
	return new Promise((resolve, reject) => {
		const xhr = new XMLHttpRequest();
		xhr.open("POST", url);
		xhr.timeout = REQUEST_TIMEOUT_MS;
		xhr.upload.onprogress = (e) =>
			e.lengthComputable && onProgress(e.loaded / e.total);
		xhr.onload = () => {
			let body: unknown;
			try {
				body = JSON.parse(xhr.responseText);
			} catch {
				body = null;
			}
			const message =
				(body as { error?: string } | null)?.error ??
				`Upload failed (${xhr.status})`;
			if (xhr.status >= 200 && xhr.status < 300) resolve(body as T);
			// Gateway and overload errors come from the network path, not the API.
			else if (xhr.status === 408 || xhr.status === 429 || xhr.status >= 500)
				reject(new NetworkError(message));
			else reject(new Error(message));
		};
		const dropped = () =>
			reject(
				new NetworkError("The connection dropped. Tap Upload to continue."),
			);
		xhr.onerror = dropped;
		xhr.ontimeout = dropped;
		xhr.send(form);
	});
}
