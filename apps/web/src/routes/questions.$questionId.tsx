import { createFileRoute, Link } from "@tanstack/react-router";
import { QuestionEditor } from "#/components/QuestionEditor";

export const Route = createFileRoute("/questions/$questionId")({
	component: BankQuestionPage,
});

function BankQuestionPage() {
	const { questionId } = Route.useParams();
	return (
		<>
			<Link to="/questions" className="text-sm font-extrabold underline">
				← Question bank
			</Link>
			<QuestionEditor id={questionId} mode="bank" />
		</>
	);
}
