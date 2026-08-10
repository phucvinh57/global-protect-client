import { Moon01, Plus, Shield01, Sun } from "@untitledui/icons";
import { useEffect, useState } from "react";
import { Button } from "react-aria-components";
import { Button as ActionButton } from "@/components/ui/button";
import type { VpnState } from "@/hooks/use-vpn";
import { useTheme } from "@/providers/theme-provider";
import { cx } from "@/utils/cx";

const statusLabel: Record<VpnState, string> = {
	authenticating: "Authenticating…",
	connecting: "Connecting…",
	connected: "Connected",
	disconnecting: "Disconnecting…",
	disconnected: "Not connected",
};

const statusDot: Record<VpnState, string> = {
	authenticating: "bg-warning-solid",
	connecting: "bg-warning-solid",
	connected: "bg-success-solid",
	disconnecting: "bg-warning-solid",
	disconnected: "bg-quaternary",
};

/** `onCreate` is omitted while a tunnel is up: the list it adds to is not shown. */
export const AppHeader = ({ state, onCreate }: { state: VpnState; onCreate?: () => void }) => (
	<header className="flex items-center gap-2 border-b border-secondary bg-primary px-3 py-2">
		<span className="flex size-6 shrink-0 items-center justify-center bg-brand-solid text-white">
			<Shield01 className="size-4" />
		</span>
		<p className="shrink-0 text-sm font-semibold text-primary">GlobalProtect</p>
		<p className="flex min-w-0 items-center gap-1.5 text-xs text-tertiary">
			<span
				className={cx(
					"size-1.5 shrink-0 rounded-full",
					statusDot[state],
					state !== "connected" && state !== "disconnected" && "animate-pulse",
				)}
			/>
			<span className="truncate">{statusLabel[state]}</span>
		</p>
		<div className="ml-auto flex shrink-0 items-center gap-1">
			<ThemeToggle />
			{onCreate && (
				<ActionButton size="sm" variant="secondary" iconLeading={Plus} onPress={onCreate}>
					New
				</ActionButton>
			)}
		</div>
	</header>
);

const ThemeToggle = () => {
	const { setTheme } = useTheme();
	const [isDark, setIsDark] = useState(() =>
		document.documentElement.classList.contains("dark-mode"),
	);

	// The provider owns the class, including the system-preference case, so the
	// element itself is the only reliable source for which mode is showing. It
	// is read once on mount too: the provider applies the class in its own
	// effect, which has already run by the time this one does.
	useEffect(() => {
		const sync = () => setIsDark(document.documentElement.classList.contains("dark-mode"));
		sync();
		const observer = new MutationObserver(sync);
		observer.observe(document.documentElement, { attributes: true, attributeFilter: ["class"] });
		return () => observer.disconnect();
	}, []);

	return (
		<Button
			aria-label={isDark ? "Switch to light theme" : "Switch to dark theme"}
			className="shrink-0 cursor-pointer p-1.5 text-fg-quaternary transition hover:bg-primary_hover hover:text-fg-secondary"
			onPress={() => setTheme(isDark ? "light" : "dark")}
		>
			{isDark ? <Sun className="size-4.5" /> : <Moon01 className="size-4.5" />}
		</Button>
	);
};
