// Copies pdf.js's decoders and fonts into public/pdfjs so scanned PDFs render in the browser.
// Run before dev and build; public/pdfjs is not committed.
import { cpSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const root = dirname(createRequire(import.meta.url).resolve("pdfjs-dist/package.json"));
const out = join(dirname(fileURLToPath(import.meta.url)), "..", "public", "pdfjs");
rmSync(out, { recursive: true, force: true });
for (const dir of ["wasm", "standard_fonts", "cmaps", "iccs"]) {
	cpSync(join(root, dir), join(out, dir), { recursive: true });
}
