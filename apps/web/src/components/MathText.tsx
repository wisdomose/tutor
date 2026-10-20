import katex from "katex";
import "katex/contrib/mhchem";
import "katex/dist/katex.min.css";

/**
 * Renders text with LaTeX between $...$ (how the converter writes maths); chemistry is
 * mhchem's \ce{…}. Fractions are drawn full size: inline they shrink too small to read
 * on a phone.
 */
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
					const tex = part.slice(1, -1);
					const html = katex.renderToString(
						/\\[dt]?frac/.test(tex) ? `\\displaystyle ${tex}` : tex,
						{ throwOnError: false },
					);
					// An equation can't wrap; a long one scrolls instead of widening the page.
					return (
						<span
							// biome-ignore lint/suspicious/noArrayIndexKey: static split of one string
							key={i}
							className="inline-block max-w-full overflow-x-auto overflow-y-hidden align-middle"
							// biome-ignore lint/security/noDangerouslySetInnerHtml: KaTeX output from its own renderer
							dangerouslySetInnerHTML={{ __html: html }}
						/>
					);
				}
				// biome-ignore lint/suspicious/noArrayIndexKey: static split of one string
				return <span key={i}>{part}</span>;
			})}
		</span>
	);
}
