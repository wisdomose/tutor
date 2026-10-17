import { type CheckReason, checkReasonLabels } from "@tutor/api-client";
import { Chip } from "./ui";

/** Why a question needs checking, as outlined chips. */
export function Reasons({ reasons }: { reasons: string[] }) {
	return (
		<>
			{reasons.map((r) => (
				<Chip
					key={r}
					fill={
						r === "conflicting_answers" || r === "no_answer"
							? "lavender"
							: "lavender-soft"
					}
				>
					{checkReasonLabels[r as CheckReason] ?? r}
				</Chip>
			))}
		</>
	);
}
