import { Copy01, Server01 } from "@untitledui/icons";
import { useEffect, useState } from "react";
import { Button as AriaButton } from "react-aria-components";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { useClipboard } from "@/hooks/use-clipboard";
import type { ConnectionInfo, VpnState } from "@/hooks/use-vpn";
import { cx } from "@/utils/cx";

type ActiveConnectionProps = {
	state: Exclude<VpnState, "disconnected">;
	name: string;
	portal: string;
	connection: ConnectionInfo | null;
	onDisconnect: () => void;
};

const headline: Record<Exclude<VpnState, "disconnected">, string> = {
	authenticating: "Authenticating",
	connecting: "Establishing tunnel",
	connected: "Connected",
	disconnecting: "Disconnecting",
};

const formatDuration = (seconds: number) => {
	const parts = [Math.floor(seconds / 3600), Math.floor(seconds / 60) % 60, seconds % 60];
	return parts.map((part) => String(part).padStart(2, "0")).join(":");
};

/**
 * The only screen shown while a tunnel exists: one connection may be active at
 * a time, so the saved list has nothing to offer until it is closed.
 */
export const ActiveConnection = ({
	state,
	name,
	portal,
	connection,
	onDisconnect,
}: ActiveConnectionProps) => {
	const connected = state === "connected";
	const elapsed = useElapsed(connected);

	return (
		<div className="flex flex-1 flex-col items-center p-4">
			<StatusRing state={state} />

			<p
				className={cx(
					"mt-3 text-xs font-semibold tracking-wide uppercase",
					connected ? "text-success-primary" : "text-brand-secondary",
				)}
			>
				{headline[state]}
			</p>
			<h2 className="mt-0.5 max-w-full truncate text-lg font-semibold text-primary">{name}</h2>
			<p className="max-w-full truncate text-xs text-tertiary">{portal}</p>
			{connected && (
				<p className="mt-2 bg-secondary px-2 py-0.5 font-mono text-xs text-secondary tabular-nums">
					{formatDuration(elapsed)}
				</p>
			)}

			{connected && connection && <ConnectionDetails connection={connection} />}
			{!connected && (
				<p className="mt-4 max-w-70 text-center text-xs text-tertiary">
					{state === "disconnecting"
						? "Closing the tunnel and restoring your routes."
						: "This can take a moment. Any prompt from the portal will appear here."}
				</p>
			)}

			<div className="mt-auto w-full pt-4">
				<Button
					className="w-full"
					variant="secondary"
					isDisabled={state === "disconnecting"}
					onPress={onDisconnect}
				>
					{state === "disconnecting" ? "Disconnecting…" : "Disconnect"}
				</Button>
			</div>
		</div>
	);
};

/** Shield inside a ring that pulses while the connection is still settling. */
const StatusRing = ({ state }: { state: Exclude<VpnState, "disconnected"> }) => {
	const connected = state === "connected";
	return (
		<div className="relative mt-2 flex size-20 items-center justify-center">
			<span
				className={cx(
					"absolute inset-0 rounded-full",
					connected ? "bg-utility-green-100" : "bg-utility-brand-100",
					state !== "connected" && state !== "disconnecting" && "animate-ping opacity-60",
				)}
			/>
			<span
				className={cx(
					"absolute inset-1.5 rounded-full",
					connected ? "bg-utility-green-100" : "bg-utility-brand-100",
				)}
			/>
			<span
				className={cx(
					"relative flex size-12 items-center justify-center rounded-full text-white shadow-lg",
					connected ? "bg-success-solid" : "bg-brand-solid",
				)}
			>
				{connected ? <ShieldCheckMark /> : <Spinner className="size-6" />}
			</span>
		</div>
	);
};

const ShieldCheckMark = () => (
	<svg viewBox="0 0 24 24" fill="none" aria-hidden className="size-6">
		<title>Connected</title>
		<path
			d="M12 2.5 4.5 5.5v6c0 4.7 3.1 8.4 7.5 10 4.4-1.6 7.5-5.3 7.5-10v-6L12 2.5Z"
			fill="currentColor"
			opacity="0.25"
		/>
		<path
			d="M12 2.5 4.5 5.5v6c0 4.7 3.1 8.4 7.5 10 4.4-1.6 7.5-5.3 7.5-10v-6L12 2.5Z"
			stroke="currentColor"
			strokeWidth="1.6"
			strokeLinejoin="round"
		/>
		<path
			d="m8.6 11.9 2.4 2.4 4.4-4.7"
			stroke="currentColor"
			strokeWidth="2"
			strokeLinecap="round"
			strokeLinejoin="round"
		/>
	</svg>
);

const ConnectionDetails = ({ connection }: { connection: ConnectionInfo }) => (
	<dl className="mt-4 w-full divide-y divide-secondary overflow-hidden bg-primary ring-1 ring-secondary">
		<DetailRow label="IP address" value={connection.addr ?? "—"} copyable />
		<DetailRow label="Interface" value={connection.ifname} />
		<DetailRow label="Gateway" value={connection.gateway} copyable />
		{connection.dns.length > 0 && <DetailRow label="DNS" value={connection.dns.join(", ")} />}
	</dl>
);

const DetailRow = ({
	label,
	value,
	copyable,
}: {
	label: string;
	value: string;
	copyable?: boolean;
}) => {
	const { copy, copied } = useClipboard();
	return (
		<div className="flex items-center gap-3 px-3 py-1.5">
			<dt className="flex shrink-0 items-center gap-1.5 text-xs text-tertiary">
				<Server01 className="size-3.5 text-fg-quaternary" />
				{label}
			</dt>
			<dd className="ml-auto min-w-0 truncate font-mono text-xs text-primary">{value}</dd>
			{copyable && value !== "—" && (
				<AriaButton
					aria-label={`Copy ${label}`}
					className="-mr-1 shrink-0 cursor-pointer p-0.5 text-fg-quaternary transition hover:bg-secondary hover:text-fg-secondary"
					onPress={() => void copy(value)}
				>
					<Copy01 className={cx("size-3.5", copied && "text-fg-success-primary")} />
				</AriaButton>
			)}
		</div>
	);
};

/** Seconds since the tunnel came up, reset whenever it goes away. */
const useElapsed = (running: boolean) => {
	const [seconds, setSeconds] = useState(0);
	useEffect(() => {
		if (!running) {
			setSeconds(0);
			return;
		}
		const started = Date.now();
		const timer = setInterval(() => setSeconds(Math.floor((Date.now() - started) / 1000)), 1000);
		return () => clearInterval(timer);
	}, [running]);
	return seconds;
};
