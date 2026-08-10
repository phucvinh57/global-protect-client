import type { FC, ReactNode } from "react";
import { Button as AriaButton, type ButtonProps as AriaButtonProps } from "react-aria-components";
import { cx, sortCx } from "@/utils/cx";

type ButtonVariant = "primary" | "secondary" | "tertiary" | "destructive";
type ButtonSize = "sm" | "md" | "lg";

const variants = sortCx({
	primary:
		"bg-brand-solid text-white shadow-xs hover:bg-brand-solid_hover pressed:bg-brand-solid_hover",
	secondary:
		"bg-primary text-secondary ring-1 ring-primary shadow-xs ring-inset hover:bg-primary_hover pressed:bg-secondary",
	tertiary: "text-tertiary hover:bg-primary_hover hover:text-secondary",
	destructive:
		"bg-error-solid text-white shadow-xs hover:bg-error-solid_hover pressed:bg-error-solid_hover",
});

const sizes = sortCx({
	sm: "gap-1 px-2 py-1 text-xs",
	md: "gap-1.5 px-3 py-1.5 text-sm",
	lg: "gap-2 px-4 py-2 text-sm",
});

type ButtonProps = AriaButtonProps & {
	variant?: ButtonVariant;
	size?: ButtonSize;
	/** Rendered before the label; pass the component, not an element. */
	iconLeading?: FC<{ className?: string }>;
	iconTrailing?: FC<{ className?: string }>;
	children?: ReactNode;
};

export const Button = ({
	variant = "primary",
	size = "md",
	iconLeading: IconLeading,
	iconTrailing: IconTrailing,
	className,
	children,
	...props
}: ButtonProps) => (
	<AriaButton
		{...props}
		className={cx(
			"inline-flex cursor-pointer items-center justify-center font-semibold whitespace-nowrap transition duration-100 ease-linear outline-focus-ring focus-visible:outline-2 focus-visible:outline-offset-2 disabled:cursor-not-allowed disabled:opacity-50 disabled:shadow-none",
			variants[variant],
			sizes[size],
			typeof className === "string" ? className : undefined,
		)}
	>
		{IconLeading && <IconLeading className="size-4 shrink-0" />}
		{children}
		{IconTrailing && <IconTrailing className="size-4 shrink-0" />}
	</AriaButton>
);
