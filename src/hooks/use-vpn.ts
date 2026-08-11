import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";

export type VpnState =
	| "authenticating"
	| "connecting"
	| "connected"
	| "disconnecting"
	| "disconnected";

export type CertificatePrompt = { fingerprint: string; details: string };
export type MfaPrompt = { message: string };
export type Gateway = { address: string; name: string };
export type ConnectionInfo = {
	ifname: string;
	addr: string | null;
	dns: string[];
	gateway: string;
};
export type NetworkCounters = {
	downloadBytes: string;
	uploadBytes: string;
	downloadPackets: string;
	uploadPackets: string;
};
export type NetworkStats = {
	downloadBytesPerSecond: number;
	uploadBytesPerSecond: number;
	session: NetworkCounters;
	lifetime: NetworkCounters;
};

export type ConnectRequest = {
	profileId: string;
	password?: string;
	otp?: string;
	remember?: boolean;
};

/** Failures worth interrupting the user for, keyed so repeats still surface. */
export type VpnError = { id: string; msg: string };

/**
 * A connection the tray asked this window to start, keyed so asking twice for
 * the same one is two requests.
 */
export type ConnectIntent = { id: string; profileId: string };

type StateEvent = { state: VpnState; profileId: string | null };
type GatewaysEvent = { list: Gateway[]; selecting: boolean };
/** A question the helper asked before this window existed. */
type PendingPrompt =
	| ({ kind: "certificate" } & CertificatePrompt)
	| ({ kind: "mfa" } & MfaPrompt)
	| { kind: "gateways"; list: Gateway[] };
type StatusResponse = {
	state: VpnState;
	profileId: string | null;
	connection: ConnectionInfo | null;
	stats: NetworkStats | null;
	pending: PendingPrompt | null;
	/** Set when the tray asked for a connection this window has yet to run. */
	requestedProfileId: string | null;
};

/**
 * Maintains the Tauri event stream and exposes the helper's command surface.
 * Debug output from the helper never reaches here — only state, prompts and
 * user-facing errors do.
 */
export const useVpn = () => {
	const [state, setState] = useState<VpnState>("disconnected");
	const [profileId, setProfileId] = useState<string | null>(null);
	const [connection, setConnection] = useState<ConnectionInfo | null>(null);
	const [stats, setStats] = useState<NetworkStats | null>(null);
	const [certificate, setCertificate] = useState<CertificatePrompt | null>(null);
	const [challenge, setChallenge] = useState<MfaPrompt | null>(null);
	// Set only while the helper is blocked waiting for a gateway choice.
	const [gatewayChoice, setGatewayChoice] = useState<Gateway[] | null>(null);
	const [error, setError] = useState<VpnError | null>(null);
	// A connection picked from the tray, which this window has to run because
	// only it can ask for a password or a passcode and report the attempt.
	const [connectIntent, setConnectIntent] = useState<ConnectIntent | null>(null);
	/** False until the backend's current state is known, so callers can tell a
	 * real transition from the window simply catching up. */
	const [ready, setReady] = useState(false);

	useEffect(() => {
		let mounted = true;
		const unlisteners = Promise.all([
			listen<StateEvent>("vpn://state", ({ payload }) => {
				if (!mounted) return;
				setState(payload.state);
				setProfileId(payload.profileId);
				if (payload.state === "disconnected") {
					setCertificate(null);
					setChallenge(null);
					setGatewayChoice(null);
					setConnection(null);
					setStats(null);
				}
			}),
			listen<ConnectionInfo>("vpn://connected", ({ payload }) => {
				if (mounted) setConnection(payload);
			}),
			listen<NetworkStats>("vpn://stats", ({ payload }) => {
				if (mounted) setStats(payload);
			}),
			listen<CertificatePrompt>("vpn://cert-untrusted", ({ payload }) => {
				if (mounted) setCertificate(payload);
			}),
			listen<MfaPrompt>("vpn://mfa-challenge", ({ payload }) => {
				if (mounted) setChallenge(payload);
			}),
			listen<GatewaysEvent>("vpn://gateways", ({ payload }) => {
				if (mounted) setGatewayChoice(payload.selecting ? payload.list : null);
			}),
			listen<{ profileId: string }>("vpn://connect-request", ({ payload }) => {
				if (mounted) setConnectIntent({ id: crypto.randomUUID(), profileId: payload.profileId });
			}),
			listen<{ msg: string }>("vpn://error", ({ payload }) => {
				if (!mounted) return;
				setError({ id: crypto.randomUUID(), msg: payload.msg });
				// The attempt is over: a question still on screen would only
				// collect an answer nothing is waiting for any more.
				setCertificate(null);
				setChallenge(null);
				setGatewayChoice(null);
			}),
		]);
		// Closing the window destroys it, so this window may be a fresh one
		// opened from the tray long after a tunnel came up — or in the middle
		// of an attempt whose question is still waiting for an answer, or one
		// built by the tray for the sole purpose of running a connection whose
		// event was emitted before the listeners above existed.
		void invoke<StatusResponse>("vpn_status").then((status) => {
			if (!mounted) return;
			setState(status.state);
			setProfileId(status.profileId);
			setConnection(status.connection);
			setStats(status.stats);
			switch (status.pending?.kind) {
				case "certificate":
					setCertificate(status.pending);
					break;
				case "mfa":
					setChallenge(status.pending);
					break;
				case "gateways":
					setGatewayChoice(status.pending.list);
					break;
			}
			if (status.requestedProfileId) {
				setConnectIntent({ id: crypto.randomUUID(), profileId: status.requestedProfileId });
			}
			setReady(true);
		});
		return () => {
			mounted = false;
			void unlisteners.then((items) => {
				items.forEach((unlisten) => {
					unlisten();
				});
			});
		};
	}, []);

	// Stats are cached by the process-level runtime, not by this webview. Keep
	// reading that snapshot while connected so a window rebuilt from the tray
	// does not depend solely on transient events emitted while no window was
	// listening.
	useEffect(() => {
		if (state !== "connected") return;
		let active = true;
		const refresh = () => {
			void invoke<NetworkStats | null>("vpn_stats").then(
				(snapshot) => {
					if (active && snapshot) setStats(snapshot);
				},
				() => {
					// The webview may be going away while this request is in flight.
				},
			);
		};
		refresh();
		const interval = window.setInterval(refresh, 1_000);
		return () => {
			active = false;
			window.clearInterval(interval);
		};
	}, [state]);

	const connect = useCallback(async (request: ConnectRequest) => {
		setError(null);
		await invoke("vpn_connect", { input: request });
	}, []);
	const disconnect = useCallback(async () => {
		await invoke("vpn_disconnect");
		setCertificate(null);
		setChallenge(null);
		setGatewayChoice(null);
	}, []);
	const trustCertificate = useCallback(async () => {
		await invoke("vpn_trust_cert");
		setCertificate(null);
	}, []);
	const rejectCertificate = useCallback(async () => {
		await invoke("vpn_reject_cert");
		setCertificate(null);
	}, []);
	const submitOtp = useCallback(async (otp: string) => {
		await invoke("vpn_submit_otp", { otp });
		setChallenge(null);
	}, []);
	const selectGateway = useCallback(async (address: string) => {
		await invoke("vpn_select_gateway", { address });
		setGatewayChoice(null);
	}, []);
	/** Told to the backend as well, so a window opened later is not handed the
	 * same request again. */
	const clearConnectIntent = useCallback(async () => {
		setConnectIntent(null);
		await invoke("vpn_clear_connect_request");
	}, []);

	return {
		state,
		ready,
		profileId,
		connection,
		stats,
		certificate,
		challenge,
		gatewayChoice,
		error,
		connectIntent,
		clearConnectIntent,
		connect,
		disconnect,
		trustCertificate,
		rejectCertificate,
		submitOtp,
		selectGateway,
	};
};
