const MAX_EDGE = 2400;
const QUALITY = 0.85;
const ASSETS = "/pdfjs";

/**
 * Renders each page of a PDF to a JPEG the same size as a shrunk photo, one at a time
 * so a long book doesn't hold every page in memory.
 */
export async function* renderPdfPages(
	file: File,
): AsyncGenerator<{ page: number; pages: number; jpeg: Blob }> {
	const task = await openPdf(file);
	const doc = await task.promise;
	try {
		for (let n = 1; n <= doc.numPages; n++) {
			const page = await doc.getPage(n);
			const base = page.getViewport({ scale: 1 });
			const viewport = page.getViewport({
				scale: MAX_EDGE / Math.max(base.width, base.height),
			});
			const canvas = document.createElement("canvas");
			canvas.width = Math.round(viewport.width);
			canvas.height = Math.round(viewport.height);
			await page.render({ canvas, viewport, background: "white" }).promise;
			page.cleanup();
			const jpeg = await new Promise<Blob>((resolve, reject) =>
				canvas.toBlob(
					(b) => (b ? resolve(b) : reject(new Error("Could not encode page"))),
					"image/jpeg",
					QUALITY,
				),
			);
			canvas.width = canvas.height = 0;
			yield { page: n, pages: doc.numPages, jpeg };
		}
	} finally {
		await task.destroy();
	}
}

/** Number of pages, for showing before upload. */
export async function countPdfPages(file: File): Promise<number> {
	const task = await openPdf(file);
	try {
		return (await task.promise).numPages;
	} finally {
		await task.destroy();
	}
}

/** pdf.js loads on first use; it's large and only runs in the browser. */
async function openPdf(file: File) {
	const pdfjs = await import("pdfjs-dist");
	const worker = await import("pdfjs-dist/build/pdf.worker.min.mjs?url");
	pdfjs.GlobalWorkerOptions.workerSrc = worker.default;
	return pdfjs.getDocument({
		data: new Uint8Array(await file.arrayBuffer()),
		wasmUrl: `${ASSETS}/wasm/`,
		standardFontDataUrl: `${ASSETS}/standard_fonts/`,
		cMapUrl: `${ASSETS}/cmaps/`,
		iccUrl: `${ASSETS}/iccs/`,
	});
}
