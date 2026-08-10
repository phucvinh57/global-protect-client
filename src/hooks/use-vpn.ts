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

export type ConnectRequest = {
	profileId: string;
	password?: string;
	otp?: string;
	remember?: boolean;
};

/** Failures worth interrupting the user for, keyed so repeats still surface. */
export type VpnError = { id: string; msg: string };

type StateEvent = { state: VpnState; profileId: string | null };
type GatewaysEvent = { list: Gateway[]; selecting: boolean };
type StatusResponse = {
	state: VpnState;
	profileId: string | null;
	connection: ConnectionInfo | null;
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
	const [certificate, setCertificate] = useState<CertificatePrompt | null>(null);
	const [challenge, setChallenge] = useState<MfaPrompt | null>(null);
	// Set only while the helper is blocked waiting for a gateway choice.
	const [gatewayChoice, setGatewayChoice] = useState<Gateway[] | null>(null);
	const [error, setError] = useState<VpnError | null>(null);
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
				}
			}),
			listen<ConnectionInfo>("vpn://connected", ({ payload }) => {
				if (mounted) setConnection(payload);
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
			listen<{ msg: string }>("vpn://error", ({ payload }) => {
				if (mounted) setError({ id: crypto.randomUUID(), msg: payload.msg });
			}),
		]);
		// The window can be reopened from the tray long after a tunnel came up.
		void invoke<StatusResponse>("vpn_status").then((status) => {
			if (!mounted) return;
			setState(status.state);
			setProfileId(status.profileId);
			setConnection(status.connection);
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

	return {
		state,
		ready,
		profileId,
		connection,
		certificate,
		challenge,
		gatewayChoice,
		error,
		connect,
		disconnect,
		trustCertificate,
		rejectCertificate,
		submitOtp,
		selectGateway,
	};
};
