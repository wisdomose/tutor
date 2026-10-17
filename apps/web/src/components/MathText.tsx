import katex from "katex";
import "katex/dist/katex.min.css";

/** Renders text with inline LaTeX between $...$ (how the converter writes maths). */
export function MathText({
	text,
	className,
}: {
	text: string;
	className?: string;
}) {
	const parts = text.split(/(\$[^$]+\$)/g);
	return (
		<span className={className}>
			{parts.map((part, i) => {
				if (part.length > 2 && part.startsWith("$") && part.endsWith("$")) {
					const html = katex.renderToString(part.slice(1, -1), {
						throwOnError: false,
					});
					// biome-ignore lint/security/noDangerouslySetInnerHtml: KaTeX output from its own renderer
					// biome-ignore lint/suspicious/noArrayIndexKey: static split of one string
					return <span key={i} dangerouslySetInnerHTML={{ __html: html }} />;
				}
				// biome-ignore lint/suspicious/noArrayIndexKey: static split of one string
				return <span key={i}>{part}</span>;
			})}
		</span>
	);
}
