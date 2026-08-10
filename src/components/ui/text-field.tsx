import type { ReactNode, Ref } from "react";
import {
	TextField as AriaTextField,
	type TextFieldProps as AriaTextFieldProps,
	FieldError,
	Input,
	Label,
	Text,
} from "react-aria-components";
import { cx } from "@/utils/cx";

export const inputClass =
	"w-full bg-primary px-2.5 py-1.5 text-sm text-primary ring-1 ring-primary ring-inset transition duration-100 ease-linear outline-none placeholder:text-placeholder focus:ring-2 focus:ring-brand disabled:cursor-not-allowed disabled:bg-secondary disabled:text-tertiary";

type TextFieldProps = AriaTextFieldProps & {
	label?: string;
	hint?: ReactNode;
	placeholder?: string;
	inputRef?: Ref<HTMLInputElement>;
	/** Renders inside the field, before the text. */
	icon?: ReactNode;
};

export const TextField = ({
	label,
	hint,
	placeholder,
	inputRef,
	icon,
	className,
	...props
}: TextFieldProps) => (
	<AriaTextField
		{...props}
		className={cx("flex flex-col gap-1", typeof className === "string" ? className : undefined)}
	>
		{label && <Label className="text-xs font-medium text-secondary">{label}</Label>}
		<div className="relative">
			{icon && (
				<span className="pointer-events-none absolute inset-y-0 left-2.5 flex items-center text-fg-quaternary">
					{icon}
				</span>
			)}
			<Input ref={inputRef} placeholder={placeholder} className={cx(inputClass, icon && "pl-9")} />
		</div>
		{hint && (
			<Text slot="description" className="text-xs text-tertiary">
				{hint}
			</Text>
		)}
		<FieldError className="text-xs text-error-primary" />
	</AriaTextField>
);
