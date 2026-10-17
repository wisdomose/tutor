import { useEffect, useState } from "react";
import { apiPath } from "#/lib/api";
import { Button } from "./ui";

const key = (pageId: string) => `tutor.rotate.${pageId}`;

/**
 * The original book photo. Phone shots of a book are often sideways, so it can be
 * rotated; the choice is remembered per page on this device. Tap to open full size.
 */
export function PagePhoto({
	pageId,
	url,
	alt,
}: {
	pageId: string;
	url: string;
	alt: string;
}) {
	const [turns, setTurns] = useState(0);
	// Natural width / height, measured once the photo loads.
	const [ratio, setRatio] = useState<number | null>(null);
	useEffect(() => {
		try {
			setTurns(Number(localStorage.getItem(key(pageId))) || 0);
		} catch {}
	}, [pageId]);
	const rotate = () => {
		const next = (turns + 1) % 4;
		setTurns(next);
		try {
			localStorage.setItem(key(pageId), String(next));
		} catch {}
	};
	const sideways = turns % 2 === 1 && ratio !== null;

	return (
		<div className="flex flex-col gap-2">
			<a
				href={apiPath(url)}
				target="_blank"
				rel="noreferrer"
				title="Open full size"
				className="block overflow-hidden rounded-[16px] border-[2.5px] border-ink bg-paper"
			>
				{/* A quarter turn swaps width and height: the box takes the rotated shape and the
				    image is sized so that, once turned, it exactly fills the box. */}
				<div
					className="relative w-full"
					style={
						sideways && ratio ? { aspectRatio: `${1} / ${ratio}` } : undefined
					}
				>
					<img
						src={apiPath(url)}
						alt={alt}
						onLoad={(e) =>
							setRatio(
								e.currentTarget.naturalWidth / e.currentTarget.naturalHeight,
							)
						}
						style={
							sideways && ratio
								? {
										position: "absolute",
										top: "50%",
										left: "50%",
										width: `${ratio * 100}%`,
										maxWidth: "none",
										transform: `translate(-50%, -50%) rotate(${turns * 90}deg)`,
									}
								: { width: "100%", transform: `rotate(${turns * 90}deg)` }
						}
					/>
				</div>
			</a>
			<Button size="sm" fill="paper" onClick={rotate}>
				Rotate ↻
			</Button>
		</div>
	);
}
