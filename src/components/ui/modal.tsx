import type { FC, ReactNode } from "react";
import { Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import { cx } from "@/utils/cx";

type ModalShellProps = {
	children: ReactNode;
	/** Coloured circle above the title. */
	icon?: FC<{ className?: string }>;
	tone?: "brand" | "error" | "warning";
	title: string;
	description?: ReactNode;
	onOpenChange?: (isOpen: boolean) => void;
	size?: "sm" | "md";
};

const tones = {
	brand: "bg-utility-brand-100 text-featured-icon-light-fg-brand ring-utility-brand-50",
	error: "bg-utility-red-100 text-featured-icon-light-fg-error ring-utility-red-50",
	warning: "bg-utility-yellow-100 text-featured-icon-light-fg-warning ring-utility-yellow-50",
};

/**
 * Every dialog in the app shares this frame so prompts that interrupt a
 * connection all look like the same thing happening.
 */
export const ModalShell = ({
	children,
	icon: Icon,
	tone = "brand",
	title,
	description,
	onOpenChange,
	size = "sm",
}: ModalShellProps) => (
	<ModalOverlay
		isOpen
		isDismissable={false}
		onOpenChange={onOpenChange}
		// The backdrop never scrolls: a wheel over it must not drag the dialog
		// off screen. Anything too tall scrolls inside the dialog instead.
		className="fixed inset-0 z-50 flex w-full items-center justify-center overflow-hidden bg-overlay/60 p-4 backdrop-blur-[6px] entering:animate-in entering:fade-in entering:duration-150 exiting:animate-out exiting:fade-out exiting:duration-100"
	>
		<Modal
			className={cx(
				"w-full outline-hidden entering:animate-in entering:zoom-in-95 entering:duration-150 entering:ease-out exiting:animate-out exiting:zoom-out-95 exiting:duration-100",
				size === "sm" ? "max-w-100" : "max-w-140",
			)}
		>
			{/* The cap is measured against the viewport rather than the overlay,
			    so it holds however the flex parent sizes itself. The heading stays
			    put and only the body scrolls. */}
			<Dialog className="flex max-h-[calc(100dvh-2rem)] flex-col overflow-hidden bg-primary p-4 shadow-xl ring-1 ring-secondary_alt outline-hidden">
				{Icon && (
					<span
						className={cx(
							"mb-3 flex size-8 shrink-0 items-center justify-center ring-4 ring-inset",
							tones[tone],
						)}
					>
						<Icon className="size-4.5" />
					</span>
				)}
				<Heading slot="title" className="shrink-0 text-md font-semibold text-primary">
					{title}
				</Heading>
				{description && <p className="mt-0.5 shrink-0 text-xs text-tertiary">{description}</p>}
				{/* min-h-0 lets a child that manages its own scrolling shrink here
				    instead of overflowing, so there is only ever one scrollbar. */}
				<div className="mt-3 flex min-h-0 flex-1 flex-col overflow-y-auto">{children}</div>
			</Dialog>
		</Modal>
	</ModalOverlay>
);

/** Right-aligned dialog footer, stacked on very narrow windows. */
export const ModalActions = ({ children }: { children: ReactNode }) => (
	<div className="mt-4 flex shrink-0 flex-col-reverse gap-2 xxs:flex-row xxs:justify-end">
		{children}
	</div>
);
