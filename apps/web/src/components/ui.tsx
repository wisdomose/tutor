// Sticker-style building blocks. See docs/design/README.md.
import { ChevronDown, type LucideIcon } from "lucide-react";
import type {
	ButtonHTMLAttributes,
	InputHTMLAttributes,
	ReactNode,
	SelectHTMLAttributes,
	TextareaHTMLAttributes,
} from "react";

const fills = {
	lime: "bg-lime",
	paper: "bg-paper",
	lavender: "bg-lavender",
	"lavender-soft": "bg-lavender-soft",
	cream: "bg-cream",
} as const;
type Fill = keyof typeof fills;

export const cx = (...parts: (string | false | undefined)[]) =>
	parts.filter(Boolean).join(" ");

/** Ink base with the coloured face lifted 4px; the face drops onto the base when pressed. */
export function Button({
	fill = "lime",
	size = "md",
	icon: Icon,
	className,
	children,
	...props
}: ButtonHTMLAttributes<HTMLButtonElement> & {
	fill?: Fill;
	size?: "sm" | "md";
	/** Shown before the label, in the label's colour. */
	icon?: LucideIcon;
}) {
	const radius = size === "sm" ? "rounded-[13px]" : "rounded-[16px]";
	return (
		<button
			type="button"
			{...props}
			className={cx(
				"group mt-1 bg-ink disabled:cursor-not-allowed",
				radius,
				className,
			)}
		>
			<span
				className={cx(
					// Disabled: flat white face and faded text, never a dimmed colour.
					"block -translate-y-1 whitespace-nowrap border-[2.5px] border-ink font-display font-extrabold text-ink transition-transform duration-75 group-active:translate-y-0 group-disabled:translate-y-0 group-disabled:bg-paper group-disabled:text-faint",
					radius,
					fills[fill],
					size === "sm" ? "px-3.5 py-2 text-sm" : "px-5 py-3.5 text-lg",
					Icon && "flex items-center justify-center gap-2",
				)}
			>
				{Icon && (
					<Icon
						aria-hidden="true"
						className={cx("shrink-0", size === "sm" ? "size-4" : "size-5")}
						strokeWidth={2.75}
					/>
				)}
				{children}
			</span>
		</button>
	);
}

export function Card({
	fill = "paper",
	className,
	children,
}: {
	fill?: Fill;
	className?: string;
	children: ReactNode;
}) {
	return (
		<div
			className={cx(
				"rounded-[18px] border-[2.5px] border-ink p-4",
				fills[fill],
				className,
			)}
		>
			{children}
		</div>
	);
}

/** Small uppercase outlined tag, e.g. HAS DIAGRAM. */
export function Chip({
	fill = "cream",
	children,
}: {
	fill?: Fill;
	children: ReactNode;
}) {
	return (
		<span
			className={cx(
				"inline-block rounded-[10px] border-2 border-ink px-2 py-0.5 text-[11px] font-extrabold uppercase tracking-wide",
				fills[fill],
			)}
		>
			{children}
		</span>
	);
}

/** Ink pill with lime text: location, counts, timers. */
export function Badge({ children }: { children: ReactNode }) {
	return (
		<span className="inline-block rounded-[8px] bg-ink px-2.5 py-0.5 text-[11px] font-extrabold uppercase tracking-wide text-lime">
			{children}
		</span>
	);
}

export function Label({ children }: { children: ReactNode }) {
	return (
		<div className="text-xs font-extrabold uppercase tracking-[1.2px]">
			{children}
		</div>
	);
}

/** Option row: letter tile + text, with the answer-revealed states. */
export function OptionRow({
	label,
	children,
	state = "idle",
}: {
	label: string;
	children: ReactNode;
	state?: "idle" | "correct" | "dimmed";
}) {
	const correct = state === "correct";
	return (
		<div
			className={cx(
				"flex items-center gap-3.5 rounded-[18px] border-ink p-3.5",
				correct ? "border-[3px] bg-lime" : "border-[2.5px] bg-paper",
				state === "dimmed" && "bg-cream opacity-45",
			)}
		>
			<div
				className={cx(
					"flex size-[34px] shrink-0 items-center justify-center rounded-[11px] font-display text-base font-extrabold",
					correct
						? "bg-ink text-lime"
						: "border-[2.5px] border-ink bg-lavender-soft",
				)}
			>
				{label}
			</div>
			<div
				className={cx(
					"flex-1 text-lg",
					correct ? "font-extrabold" : "font-bold",
				)}
			>
				{children}
			</div>
			{correct && <Chip fill="paper">Correct</Chip>}
		</div>
	);
}

/** Inputs are wells pressed into the card; see `.field` in styles.css. */
const control = "field";

export function Field({
	label,
	children,
}: {
	label: string;
	children: ReactNode;
}) {
	return (
		// biome-ignore lint/a11y/noLabelWithoutControl: the control is passed in as children
		<label className="flex flex-col gap-1.5">
			<span className="text-xs font-extrabold uppercase tracking-[1.2px]">
				{label}
			</span>
			{children}
		</label>
	);
}

export function TextInput(props: InputHTMLAttributes<HTMLInputElement>) {
	return <input {...props} className={cx(control, props.className)} />;
}

export function TextArea(props: TextareaHTMLAttributes<HTMLTextAreaElement>) {
	return (
		<textarea rows={2} {...props} className={cx(control, props.className)} />
	);
}

/** A native select in the same well, with a chevron. */
export function Select({
	className,
	...props
}: SelectHTMLAttributes<HTMLSelectElement>) {
	return (
		<div className={cx("relative", className)}>
			<select
				{...props}
				className={cx(control, "cursor-pointer appearance-none truncate pr-10")}
			/>
			<ChevronDown
				aria-hidden="true"
				className="pointer-events-none absolute top-1/2 right-3 size-5 -translate-y-1/2"
				strokeWidth={3}
			/>
		</div>
	);
}

const statusStyles: Record<string, { fill: Fill; label: string }> = {
	queued: { fill: "cream", label: "Waiting" },
	processing: { fill: "lavender", label: "Reading…" },
	done: { fill: "lime", label: "Done" },
	failed: { fill: "paper", label: "Failed" },
};

/** Page status: waiting / reading / done / failed. */
export function StatusChip({ status }: { status: string }) {
	const s = statusStyles[status] ?? { fill: "cream" as Fill, label: status };
	return <Chip fill={s.fill}>{s.label}</Chip>;
}

export function Spinner({ label = "Loading…" }: { label?: string }) {
	return (
		<div className="flex items-center gap-2 py-6 text-sm font-extrabold text-muted">
			<span className="inline-block size-3 animate-pulse rounded-full border-2 border-ink bg-lime" />
			{label}
		</div>
	);
}

export function ErrorNote({ error }: { error: unknown }) {
	return (
		<div className="rounded-[16px] border-[2.5px] border-ink bg-paper p-3 text-sm font-bold">
			{error instanceof Error ? error.message : String(error)}
		</div>
	);
}

export function Empty({
	title,
	children,
}: {
	title: string;
	children?: ReactNode;
}) {
	return (
		<Card fill="lavender-soft" className="text-center">
			<div className="font-display text-xl font-extrabold">{title}</div>
			{children && (
				<div className="mt-1 text-sm font-bold text-muted">{children}</div>
			)}
		</Card>
	);
}

export function PageTitle({
	label,
	children,
}: {
	label?: string;
	children: ReactNode;
}) {
	return (
		<header className="pb-1">
			{label && <Label>{label}</Label>}
			<h1 className="font-display text-[28px] leading-tight font-extrabold">
				{children}
			</h1>
		</header>
	);
}

/** "Biology · Page 5 · Q8" — where to find a question in the book. */
export function Location({
	subject,
	page,
	number,
}: {
	subject: string;
	page?: string | null;
	number?: string;
}) {
	return (
		<Badge>
			{[subject, page ? `Page ${page}` : "Page ?", number ? `Q${number}` : null]
				.filter(Boolean)
				.join(" · ")}
		</Badge>
	);
}
