import { createFileRoute } from "@tanstack/react-router";

// Forwards /api/* from the browser to the Rust API, so the API address is read
// at runtime (API_URL) and the browser only ever talks to this site.
async function forward({
	request,
	params,
}: {
	request: Request;
	params: { _splat?: string };
}) {
	const apiUrl = process.env.API_URL ?? "http://localhost:8080";
	const incoming = new URL(request.url);
	const target = `${apiUrl}/${params._splat ?? ""}${incoming.search}`;
	const headers = new Headers(request.headers);
	for (const h of ["host", "connection", "content-length", "accept-encoding"])
		headers.delete(h);
	const hasBody = request.method !== "GET" && request.method !== "HEAD";
	const res = await fetch(target, {
		method: request.method,
		headers,
		body: hasBody ? request.body : undefined,
		// Required by Node's fetch to stream a request body (photo uploads).
		...(hasBody ? { duplex: "half" } : {}),
	} as RequestInit);
	const out = new Headers(res.headers);
	for (const h of [
		"content-encoding",
		"content-length",
		"transfer-encoding",
		"connection",
	])
		out.delete(h);
	return new Response(res.body, { status: res.status, headers: out });
}

export const Route = createFileRoute("/api/$")({
	server: {
		handlers: { GET: forward, POST: forward, PUT: forward, DELETE: forward },
	},
});
