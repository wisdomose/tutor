import { Link } from "@tanstack/react-router";
import type { QuestionSummary } from "@tutor/api-client";
import { MathText } from "./MathText";
import { Reasons } from "./Reasons";
import { Chip, Location } from "./ui";

/** One question in a list, linking to its review or bank screen. */
export function QuestionRow({
	q,
	to,
}: {
	q: QuestionSummary;
	to: "/review/$questionId" | "/questions/$questionId";
}) {
	return (
		<Link
			to={to}
			params={{ questionId: q.id }}
			className="rounded-[16px] border-[2.5px] border-ink bg-paper p-3"
		>
			<div className="mb-1.5 flex flex-wrap items-center gap-1.5">
				<Location
					subject={q.source ?? q.subject}
					page={q.page_number}
					number={q.number}
				/>
				{q.needs_check && !q.resolved_at && (
					<Reasons reasons={q.check_reasons} />
				)}
				{q.resolved_at && <Chip fill="lime">Checked</Chip>}
			</div>
			<MathText
				text={q.body}
				className="line-clamp-2 text-[15px] leading-snug font-bold"
			/>
		</Link>
	);
}
