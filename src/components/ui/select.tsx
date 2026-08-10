import { Check, ChevronDown } from "@untitledui/icons";
import {
	Select as AriaSelect,
	Button,
	Label,
	ListBox,
	ListBoxItem,
	Popover,
	SelectValue,
} from "react-aria-components";
import { inputClass } from "@/components/ui/text-field";
import { cx } from "@/utils/cx";

type SelectProps = {
	label?: string;
	items: { id: string; label: string }[];
	selectedKey: string;
	onSelectionChange: (key: string) => void;
	hint?: string;
};

export const Select = ({ label, items, selectedKey, onSelectionChange, hint }: SelectProps) => (
	<AriaSelect
		className="flex flex-col gap-1"
		selectedKey={selectedKey}
		onSelectionChange={(key) => onSelectionChange(String(key))}
	>
		{label && <Label className="text-xs font-medium text-secondary">{label}</Label>}
		<Button
			className={cx(inputClass, "flex cursor-pointer items-center justify-between gap-2 text-left")}
		>
			<SelectValue className="truncate" />
			<ChevronDown className="size-4 shrink-0 text-fg-quaternary" />
		</Button>
		{hint && <p className="text-xs text-tertiary">{hint}</p>}
		<Popover
			offset={4}
			className="w-(--trigger-width) overflow-auto bg-primary py-1 shadow-lg ring-1 ring-secondary_alt entering:animate-in entering:fade-in entering:zoom-in-95 entering:duration-100 exiting:animate-out exiting:fade-out exiting:duration-75"
		>
			<ListBox className="outline-hidden">
				{items.map((item) => (
					<ListBoxItem
						key={item.id}
						id={item.id}
						textValue={item.label}
						className="flex cursor-pointer items-center justify-between gap-2 px-2.5 py-1.5 text-sm text-primary outline-hidden hover:bg-primary_hover focus:bg-primary_hover"
					>
						{({ isSelected }) => (
							<>
								<span className="truncate">{item.label}</span>
								{isSelected && <Check className="size-4 shrink-0 text-fg-brand-primary" />}
							</>
						)}
					</ListBoxItem>
				))}
			</ListBox>
		</Popover>
	</AriaSelect>
);
