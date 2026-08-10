import { File02, XClose } from "@untitledui/icons";
import type { ReactNode } from "react";
import { Button as AriaButton } from "react-aria-components";
import { cx } from "@/utils/cx";

/** Certificates and scripts live outside the app, so paths are picked, not typed. */
export type FileFilter = { name: string; extensions: string[] };

type FileFieldProps = {
	label: string;
	/** Absolute path, or "" when nothing is chosen. */
	value: string;
	onChange: (value: string) => void;
	/** Shown in place of a path while the field is empty. */
	placeholder?: string;
	hint?: ReactNode;
	filters?: FileFilter[];
};

const fileName = (path: string) => path.split(/[\\/]/).pop() || path;

export const FileField = ({
	label,
	value,
	onChange,
	placeholder = "No file selected",
	hint,
	filters,
}: FileFieldProps) => {
	const browse = async () => {
		// Imported here so the browser-only mock page never pulls in the plugin.
		const { open } = await import("@tauri-apps/plugin-dialog");
		const picked = await open({ multiple: false, directory: false, title: label, filters });
		if (typeof picked === "string") {
			onChange(picked);
		}
	};

	return (
		<div className="flex flex-col gap-1">
			<span className="text-xs font-medium text-secondary">{label}</span>
			<div className="flex items-center gap-1.5">
				<AriaButton
					onPress={browse}
					// The full path rarely fits, so the button shows the file name and
					// keeps the path itself in the tooltip and the accessible name.
					aria-label={value ? `${label}: ${value}` : `Choose a file for ${label}`}
					className="flex min-w-0 flex-1 cursor-pointer items-center gap-2 bg-primary px-2.5 py-1.5 text-sm ring-1 ring-primary ring-inset transition duration-100 ease-linear outline-none hover:bg-primary_hover focus-visible:ring-2 focus-visible:ring-brand"
				>
					<File02 className="size-4 shrink-0 text-fg-quaternary" />
					<span
						title={value || undefined}
						className={cx("truncate text-left", value ? "text-primary" : "text-placeholder")}
					>
						{value ? fileName(value) : placeholder}
					</span>
				</AriaButton>
				{value && (
					<AriaButton
						onPress={() => onChange("")}
						aria-label={`Clear ${label}`}
						className="cursor-pointer rounded-lg p-2.5 text-fg-quaternary transition duration-100 ease-linear outline-none hover:bg-primary_hover hover:text-fg-secondary focus-visible:ring-2 focus-visible:ring-brand"
					>
						<XClose className="size-5" />
					</AriaButton>
				)}
			</div>
			{hint && <p className="text-sm text-tertiary">{hint}</p>}
		</div>
	);
};
