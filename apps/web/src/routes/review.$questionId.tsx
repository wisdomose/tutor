import { createFileRoute, Link } from "@tanstack/react-router";
import { QuestionEditor } from "#/components/QuestionEditor";

export const Route = createFileRoute("/review/$questionId")({
	component: ReviewQuestionPage,
});

function ReviewQuestionPage() {
	const { questionId } = Route.useParams();
	return (
		<>
			<Link to="/review" className="text-sm font-extrabold underline">
				← Review queue
			</Link>
			<QuestionEditor id={questionId} mode="review" />
		</>
	);
}
