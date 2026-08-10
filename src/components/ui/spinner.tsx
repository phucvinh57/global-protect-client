import { cx } from "@/utils/cx";

/** Indeterminate ring. Sized by the caller through `className`. */
export const Spinner = ({ className }: { className?: string }) => (
	<svg viewBox="0 0 24 24" fill="none" aria-hidden className={cx("size-5 animate-spin", className)}>
		<title>Working</title>
		<circle cx="12" cy="12" r="9.5" stroke="currentColor" strokeWidth="2.5" opacity="0.25" />
		<path
			d="M12 2.5a9.5 9.5 0 0 1 9.5 9.5"
			stroke="currentColor"
			strokeWidth="2.5"
			strokeLinecap="round"
		/>
	</svg>
);
