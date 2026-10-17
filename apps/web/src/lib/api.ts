import { createApiClient } from "@tutor/api-client";

/** Browser calls go through this site's /api forwarder (see routes/api/$.ts). */
export const api = createApiClient("/api");

/** Unwraps an openapi-fetch result, turning API errors into thrown Errors for react-query. */
export async function unwrap<T>(
	promise: Promise<{ data?: T; error?: unknown; response: Response }>,
): Promise<T> {
	const { data, error, response } = await promise;
	if (error !== undefined || !response.ok) {
		const message =
			error && typeof error === "object" && "error" in error
				? String((error as { error: unknown }).error)
				: `Request failed (${response.status})`;
		throw new Error(message);
	}
	return data as T;
}

/** Paths returned by the API (photo_url, svg_url) are relative to it. */
export const apiPath = (path: string) => `/api${path}`;
