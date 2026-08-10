import { Server01 } from "@untitledui/icons";
import { useState } from "react";
import { Radio, RadioGroup } from "react-aria-components";
import { Button } from "@/components/ui/button";
import { ModalActions, ModalShell } from "@/components/ui/modal";
import type { Gateway } from "@/hooks/use-vpn";

type GatewayDialogProps = {
	gateways: Gateway[];
	onCancel: () => void;
	onSelect: (address: string) => void;
};

/** Shown when the portal offers more than one gateway. */
export const GatewayDialog = ({ gateways, onCancel, onSelect }: GatewayDialogProps) => {
	const [address, setAddress] = useState(gateways[0]?.address ?? "");

	return (
		<ModalShell
			icon={Server01}
			title="Choose a gateway"
			description="This portal offers several gateways. Pick the one you are allowed to use."
		>
			<RadioGroup
				aria-label="Gateway"
				value={address}
				onChange={setAddress}
				className="flex max-h-64 flex-col gap-1.5 overflow-y-auto"
			>
				{gateways.map((gateway) => (
					<Radio
						key={gateway.address}
						value={gateway.address}
						className="group flex cursor-pointer items-start gap-2 p-2 ring-1 ring-secondary transition duration-100 ease-linear outline-hidden selected:bg-brand-primary_alt selected:ring-2 selected:ring-brand focus-visible:ring-2 focus-visible:ring-brand"
					>
						<span className="mt-0.5 flex size-3.5 shrink-0 items-center justify-center rounded-full ring-1 ring-primary ring-inset group-selected:bg-brand-solid group-selected:ring-brand-solid">
							<span className="size-1 rounded-full bg-white opacity-0 group-selected:opacity-100" />
						</span>
						<span className="min-w-0">
							<span className="block truncate text-xs font-medium text-primary">
								{gateway.name}
							</span>
							<span className="block truncate font-mono text-xs text-tertiary">
								{gateway.address}
							</span>
						</span>
					</Radio>
				))}
			</RadioGroup>
			<ModalActions>
				<Button variant="secondary" onPress={onCancel}>
					Cancel
				</Button>
				<Button isDisabled={!address} onPress={() => onSelect(address)}>
					Connect
				</Button>
			</ModalActions>
		</ModalShell>
	);
};
