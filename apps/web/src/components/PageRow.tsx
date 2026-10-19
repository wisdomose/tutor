import { Link } from "@tanstack/react-router";
import type { PageSummary } from "@tutor/api-client";
import { Chip, StatusChip } from "./ui";

/** One uploaded photo in a list: where it is in the book, its status and counts. */
export function PageRow({ page }: { page: PageSummary }) {
	return (
		<Link
			to="/uploads/$pageId"
			params={{ pageId: page.id }}
			className="flex items-center gap-3 rounded-[16px] border-[2.5px] border-ink bg-paper p-3"
		>
			<div className="flex size-12 shrink-0 flex-col items-center justify-center rounded-[13px] border-[2.5px] border-ink bg-lavender-soft font-display leading-none font-extrabold">
				<span className="text-[10px] tracking-wide uppercase">Page</span>
				<span className="text-lg">{page.page_number ?? "?"}</span>
			</div>
			<div className="min-w-0 flex-1">
				<div className="flex flex-wrap items-center gap-1.5">
					<span className="text-[15px] font-extrabold">{page.source}</span>
					<StatusChip status={page.status} />
					{page.flagged_count > 0 && (
						<Chip fill="lavender">{page.flagged_count} to check</Chip>
					)}
				</div>
				<div className="mt-0.5 truncate text-xs font-bold text-muted">
					{page.status === "done"
						? `${page.question_count} question${page.question_count === 1 ? "" : "s"}${
								page.waiting_for_next_page
									? " · last one continues on the next page"
									: ""
							}`
						: page.status === "failed" || page.error
							? (page.error ?? "Failed")
							: `Uploaded ${new Date(page.uploaded_at).toLocaleString([], { dateStyle: "medium", timeStyle: "short" })}`}
				</div>
			</div>
		</Link>
	);
}
