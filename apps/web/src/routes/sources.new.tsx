import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { SourceForm } from "#/components/SourceForm";
import { Card, PageTitle } from "#/components/ui";
import { SOURCE_KEY } from "#/lib/source";

export const Route = createFileRoute("/sources/new")({
	component: NewSourcePage,
});

/** Its own page, so the phone's back button leaves it; a new source is selected for upload. */
function NewSourcePage() {
	const navigate = useNavigate();
	return (
		<>
			<PageTitle>New book or paper</PageTitle>
			<Card fill="lavender">
				<SourceForm
					onCreated={(s) => {
						try {
							localStorage.setItem(SOURCE_KEY, s.id);
						} catch {}
						navigate({ to: "/" });
					}}
					onCancel={() => navigate({ to: "/" })}
				/>
			</Card>
		</>
	);
}
