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
				{q.kind === "theory" && <Chip fill="paper">Theory</Chip>}
				{q.needs_check && !q.resolved_at && (
					<Reasons reasons={q.check_reasons} />
				)}
				{q.resolved_at && <Chip fill="lime">Checked</Chip>}
			</div>
			<MathText
				text={preview(q.body || q.first_part || "")}
				className="block text-[15px] leading-snug font-bold"
			/>
		</Link>
	);
}

const PREVIEW_CHARS = 140;

/**
 * Shortens a question for a list. It's cut between words, never inside an equation:
 * a CSS line clamp slices rendered maths and leaves stray subscripts behind.
 */
function preview(text: string) {
	if (text.length <= PREVIEW_CHARS) return text;
	let out = "";
	for (const part of text.split(/(\$[^$]+\$)/g)) {
		if (out.length + part.length <= PREVIEW_CHARS) {
			out += part;
			continue;
		}
		if (!part.startsWith("$")) {
			const room = part.slice(0, PREVIEW_CHARS - out.length);
			out += room.slice(0, Math.max(0, room.lastIndexOf(" ")));
		}
		break;
	}
	return `${out.trimEnd()}…`;
}
