import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api, unwrap } from "#/lib/api";
import { Button, Card, Label } from "./ui";

const time = (iso: string) =>
	new Date(iso).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });

/** What the converter is doing, with a pause/resume switch. */
export function QueuePanel() {
	const qc = useQueryClient();
	const queue = useQuery({
		queryKey: ["queue"],
		queryFn: () => unwrap(api.GET("/queue")),
		refetchInterval: 5_000,
	});
	const toggle = useMutation({
		mutationFn: (pause: boolean) =>
			unwrap(pause ? api.POST("/queue/pause") : api.POST("/queue/resume")),
		onSuccess: (data) => qc.setQueryData(["queue"], data),
	});
	const q = queue.data;
	if (!q) return null;

	const working = q.queued + q.processing;
	let title: string;
	let detail: string;
	if (q.paused) {
		title = "Paused";
		detail = `${working} page${working === 1 ? "" : "s"} waiting. Resume when you're not using Claude yourself.`;
	} else if (q.limited_until) {
		title = `Claude limit reached — resumes ${time(q.limited_until)}`;
		detail = `${working} page${working === 1 ? "" : "s"} waiting. Nothing is lost; it carries on by itself.`;
	} else if (working > 0) {
		title = `Reading ${working} page${working === 1 ? "" : "s"}`;
		detail = "About a minute per page, one at a time. You can close this page.";
	} else if (q.diagrams_pending > 0) {
		title = `Drawing ${q.diagrams_pending} diagram${q.diagrams_pending === 1 ? "" : "s"}`;
		detail = "Questions are saved; diagrams follow shortly.";
	} else {
		title = "All caught up";
		detail =
			q.failed > 0
				? `${q.failed} page${q.failed === 1 ? "" : "s"} failed — see Uploads.`
				: "Nothing waiting.";
	}
	const paused = q.paused || !!q.limited_until;

	return (
		<Card fill={paused ? "lavender" : working > 0 ? "lavender-soft" : "paper"}>
			<div className="flex items-center gap-3">
				<div className="min-w-0 flex-1">
					<Label>Converter</Label>
					<div className="font-display text-xl leading-tight font-extrabold">
						{title}
					</div>
					<div className="mt-0.5 text-sm font-bold text-muted">{detail}</div>
				</div>
				<Button
					size="sm"
					fill={paused ? "lime" : "paper"}
					disabled={toggle.isPending}
					onClick={() => toggle.mutate(!paused)}
				>
					{paused ? "Resume" : "Pause"}
				</Button>
			</div>
		</Card>
	);
}
