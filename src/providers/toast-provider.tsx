import { AlertCircle, CheckCircle, InfoCircle, XClose } from "@untitledui/icons";
import type { ReactNode } from "react";
import {
	createContext,
	useCallback,
	useContext,
	useEffect,
	useMemo,
	useRef,
	useState,
} from "react";
import { Button } from "react-aria-components";
import { Spinner } from "@/components/ui/spinner";
import { cx } from "@/utils/cx";

export type ToastTone = "info" | "success" | "error" | "pending";

export type ToastOptions = {
	tone?: ToastTone;
	title: string;
	description?: string;
};

type Toast = ToastOptions & { id: string; tone: ToastTone };

type ToastContextType = {
	/** Returns the toast id so a pending toast can be resolved later. */
	show: (options: ToastOptions) => string;
	update: (id: string, options: ToastOptions) => void;
	dismiss: (id: string) => void;
};

const ToastContext = createContext<ToastContextType | undefined>(undefined);

export const useToast = (): ToastContextType => {
	const context = useContext(ToastContext);
	if (!context) {
		throw new Error("useToast must be used within a ToastProvider");
	}
	return context;
};

/** Pending toasts wait for whatever they describe; the rest time out. */
const lifetime = (tone: ToastTone) => (tone === "pending" ? null : tone === "error" ? 7000 : 4000);

export const ToastProvider = ({ children }: { children: ReactNode }) => {
	const [toasts, setToasts] = useState<Toast[]>([]);
	const timers = useRef(new Map<string, ReturnType<typeof setTimeout>>());

	const dismiss = useCallback((id: string) => {
		const timer = timers.current.get(id);
		if (timer) {
			clearTimeout(timer);
			timers.current.delete(id);
		}
		setToasts((current) => current.filter((toast) => toast.id !== id));
	}, []);

	const schedule = useCallback(
		(id: string, tone: ToastTone) => {
			const previous = timers.current.get(id);
			if (previous) clearTimeout(previous);
			const duration = lifetime(tone);
			if (duration === null) {
				timers.current.delete(id);
				return;
			}
			timers.current.set(
				id,
				setTimeout(() => dismiss(id), duration),
			);
		},
		[dismiss],
	);

	const show = useCallback(
		({ tone = "info", ...options }: ToastOptions) => {
			const id = crypto.randomUUID();
			setToasts((current) => [...current.slice(-2), { ...options, tone, id }]);
			schedule(id, tone);
			return id;
		},
		[schedule],
	);

	const update = useCallback(
		(id: string, { tone = "info", ...options }: ToastOptions) => {
			setToasts((current) =>
				current.map((toast) => (toast.id === id ? { ...toast, ...options, tone } : toast)),
			);
			schedule(id, tone);
		},
		[schedule],
	);

	useEffect(() => {
		const pending = timers.current;
		return () => {
			pending.forEach(clearTimeout);
			pending.clear();
		};
	}, []);

	// Stable so effects elsewhere can depend on the toast API without re-running
	// every time a toast appears.
	const api = useMemo(() => ({ show, update, dismiss }), [show, update, dismiss]);

	return (
		<ToastContext.Provider value={api}>
			{children}
			<ToastViewport toasts={toasts} onDismiss={dismiss} />
		</ToastContext.Provider>
	);
};

const icons = {
	info: <InfoCircle className="size-5 text-fg-brand-primary" />,
	success: <CheckCircle className="size-5 text-fg-success-primary" />,
	error: <AlertCircle className="size-5 text-fg-error-primary" />,
	pending: <Spinner className="size-5 text-fg-brand-primary" />,
};

const ToastViewport = ({
	toasts,
	onDismiss,
}: {
	toasts: Toast[];
	onDismiss: (id: string) => void;
}) => (
	<div className="pointer-events-none fixed inset-x-0 bottom-3 z-100 flex flex-col items-end gap-1.5 px-3">
		{toasts.map((toast) => (
			<div
				key={toast.id}
				role={toast.tone === "error" ? "alert" : "status"}
				className={cx(
					"pointer-events-auto flex w-full max-w-92 items-start gap-2.5 bg-primary p-2.5 shadow-lg ring-1 ring-secondary_alt",
					"animate-in fade-in slide-in-from-bottom-3 duration-200 ease-out",
				)}
			>
				<span className="mt-px shrink-0">{icons[toast.tone]}</span>
				<div className="min-w-0 flex-1">
					<p className="text-xs font-semibold text-primary">{toast.title}</p>
					{toast.description && (
						<p className="text-xs wrap-break-word text-tertiary">{toast.description}</p>
					)}
				</div>
				<Button
					aria-label="Dismiss"
					className="-m-1 shrink-0 cursor-pointer rounded-md p-1 text-fg-quaternary transition hover:bg-primary_hover hover:text-fg-tertiary"
					onPress={() => onDismiss(toast.id)}
				>
					<XClose className="size-4" />
				</Button>
			</div>
		))}
	</div>
);
