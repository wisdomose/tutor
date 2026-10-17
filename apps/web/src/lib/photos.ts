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

/** Uploads photos for one subject, reporting progress 0–1 (fetch can't report upload progress). */
export function uploadPhotos(
	subjectId: string,
	photos: Blob[],
	onProgress: (fraction: number) => void,
): Promise<PageSummary[]> {
	const form = new FormData();
	form.append("subject_id", subjectId);
	photos.forEach((p, i) => {
		form.append("photos", p, `page-${i + 1}.jpg`);
	});
	return new Promise((resolve, reject) => {
		const xhr = new XMLHttpRequest();
		xhr.open("POST", "/api/pages");
		xhr.upload.onprogress = (e) =>
			e.lengthComputable && onProgress(e.loaded / e.total);
		xhr.onload = () => {
			let body: unknown;
			try {
				body = JSON.parse(xhr.responseText);
			} catch {
				body = null;
			}
			if (xhr.status >= 200 && xhr.status < 300) resolve(body as PageSummary[]);
			else
				reject(
					new Error(
						(body as { error?: string } | null)?.error ??
							`Upload failed (${xhr.status})`,
					),
				);
		};
		xhr.onerror = () =>
			reject(new Error("Upload failed — check your connection and try again"));
		xhr.send(form);
	});
}
