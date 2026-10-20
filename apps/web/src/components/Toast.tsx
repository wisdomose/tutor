import { Check, TriangleAlert, X } from "lucide-react";
import {
	createContext,
	type ReactNode,
	useCallback,
	useContext,
	useEffect,
	useMemo,
	useRef,
	useState,
} from "react";
import { cx } from "./ui";

type Kind = "success" | "error";
type Item = { id: number; message: ReactNode; kind: Kind };

/** Shows a short note at the bottom of the screen: `toast("Saved")`, `toast("Couldn't save", "error")`. */
export type ShowToast = (message: ReactNode, kind?: Kind) => void;

const ToastContext = createContext<ShowToast>(() => {});

export const useToast = () => useContext(ToastContext);

/** How long each kind stays up unless closed; errors stay longer to be read. */
const LIFETIME: Record<Kind, number> = { success: 3000, error: 7000 };

/** Wrap the app once; any page can then call `useToast()`. */
export function ToastProvider({ children }: { children: ReactNode }) {
	const [items, setItems] = useState<Item[]>([]);
	const next = useRef(0);
	const close = useCallback(
		(id: number) => setItems((all) => all.filter((t) => t.id !== id)),
		[],
	);
	const show = useCallback<ShowToast>((message, kind = "success") => {
		const id = next.current++;
		// Keep at most three on screen.
		setItems((all) => [...all.slice(-2), { id, message, kind }]);
	}, []);
	const value = useMemo(() => show, [show]);

	return (
		<ToastContext.Provider value={value}>
			{children}
			<div className="pointer-events-none fixed inset-x-0 bottom-[max(1.25rem,env(safe-area-inset-bottom))] z-30 flex flex-col items-center gap-2 px-20">
				{items.map((t) => (
					<ToastItem key={t.id} item={t} onClose={() => close(t.id)} />
				))}
			</div>
		</ToastContext.Provider>
	);
}

function ToastItem({ item, onClose }: { item: Item; onClose: () => void }) {
	// The page re-renders while it polls; keep the first timer rather than restarting it.
	const closeRef = useRef(onClose);
	closeRef.current = onClose;
	useEffect(() => {
		const t = setTimeout(() => closeRef.current(), LIFETIME[item.kind]);
		return () => clearTimeout(t);
	}, [item.kind]);
	const Icon = item.kind === "error" ? TriangleAlert : Check;
	return (
		<output
			className={cx(
				"toast pointer-events-auto flex max-w-full items-center gap-2 rounded-full border-[2.5px] border-ink py-2 pr-2 pl-2.5 font-display text-base font-extrabold",
				item.kind === "error" ? "bg-paper text-ink" : "bg-ink text-lime",
			)}
		>
			<span
				className={cx(
					"flex size-6 shrink-0 items-center justify-center rounded-full",
					item.kind === "error" ? "bg-lavender" : "bg-lime text-ink",
				)}
			>
				<Icon aria-hidden="true" className="size-4" strokeWidth={3} />
			</span>
			<span className="min-w-0 truncate">{item.message}</span>
			<button
				type="button"
				aria-label="Close"
				onClick={onClose}
				className="flex size-7 shrink-0 items-center justify-center rounded-full opacity-80 hover:opacity-100"
			>
				<X aria-hidden="true" className="size-4" strokeWidth={3} />
			</button>
		</output>
	);
}
