import { Check } from "@untitledui/icons";
import type { ReactNode } from "react";
import {
	Checkbox as AriaCheckbox,
	type CheckboxProps as AriaCheckboxProps,
} from "react-aria-components";
import { cx } from "@/utils/cx";

type CheckboxProps = Omit<AriaCheckboxProps, "children"> & {
	label: ReactNode;
	hint?: ReactNode;
};

export const Checkbox = ({ label, hint, className, ...props }: CheckboxProps) => (
	<AriaCheckbox
		{...props}
		className={cx(
			"group flex items-start gap-2 disabled:cursor-not-allowed disabled:opacity-60",
			typeof className === "string" ? className : undefined,
		)}
	>
		<span className="mt-0.5 flex size-4 shrink-0 items-center justify-center bg-primary ring-1 ring-primary ring-inset transition duration-100 ease-linear group-selected:bg-brand-solid group-selected:ring-brand-solid group-focus-visible:outline-2 group-focus-visible:outline-offset-2 group-focus-visible:outline-focus-ring">
			<Check className="size-3 text-white opacity-0 transition-opacity duration-100 group-selected:opacity-100" />
		</span>
		<span className="flex flex-col">
			<span className="text-xs font-medium text-secondary">{label}</span>
			{hint && <span className="text-xs text-tertiary">{hint}</span>}
		</span>
	</AriaCheckbox>
);
