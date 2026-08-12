import { useCallback, useEffect, useRef, useState } from "react";
import { ActiveConnection } from "@/components/active-connection";
import { AppHeader } from "@/components/app-header";
import { CertTrustDialog } from "@/components/cert-trust-dialog";
import { ConnectionForm } from "@/components/connection-form";
import { ConnectionList } from "@/components/connection-list";
import { CredentialsDialog } from "@/components/credentials-dialog";
import { GatewayDialog } from "@/components/gateway-dialog";
import { MfaDialog } from "@/components/mfa-dialog";
import { emptyProfile, type Profile, type ProfileDraft, useProfiles } from "@/hooks/use-profiles";
import { useVpn, type VpnState } from "@/hooks/use-vpn";
import { useToast } from "@/providers/toast-provider";

const message = (reason: unknown) =>
	reason instanceof Error ? reason.message : String(reason ?? "");

export const ConnectionPage = () => {
	const vpn = useVpn();
	const profiles = useProfiles();
	const toast = useToast();

	const [editing, setEditing] = useState<ProfileDraft | Profile | null>(null);
	// Set while a connection attempt is waiting for the user to type a password.
	const [askingCredentials, setAskingCredentials] = useState<Profile | null>(null);
	// The toast that follows one attempt from "Connecting…" to its outcome.
	const attempt = useRef<string | null>(null);
	const previousState = useRef<VpnState>("disconnected");
	// The window can open onto a tunnel that was already up; that is not a
	// transition and must not announce itself.
	const primed = useRef(false);

	const active = profiles.profiles.find((profile) => profile.id === vpn.profileId) ?? null;

	const run = useCallback(
		async (
			profile: Profile,
			credentials?: { password: string; otp: string; remember: boolean },
		) => {
			try {
				await vpn.connect({ profileId: profile.id, ...credentials });
			} catch (reason) {
				const id = attempt.current;
				attempt.current = null;
				const description = message(reason);
				if (id) {
					toast.update(id, { tone: "error", title: "Could not connect", description });
				} else {
					toast.show({ tone: "error", title: "Could not connect", description });
				}
			}
		},
		[toast, vpn.connect],
	);

	const start = useCallback(
		(profile: Profile) => {
			attempt.current = toast.show({
				tone: "pending",
				title: `Connecting to ${profile.name}`,
				description: profile.portal,
			});
			// Anything the keyring cannot answer is asked for once, up front — a
			// one-time passcode included, since the keyring can never hold one.
			if (profile.hasSavedPassword && !profile.requiresOtp) {
				void run(profile);
			} else {
				setAskingCredentials(profile);
			}
		},
		[run, toast],
	);

	const cancelCredentials = useCallback(() => {
		setAskingCredentials(null);
		if (attempt.current) {
			toast.dismiss(attempt.current);
			attempt.current = null;
		}
	}, [toast]);

	// A connection picked from the tray runs exactly like one picked from the
	// list: the toast follows the attempt, and anything the keyring cannot
	// answer — a one-time passcode above all — is still asked for here. The
	// window may have been built for this alone, so the request arrives with
	// the rest of the backend's state and waits for the saved connections it
	// names to load.
	const handledIntent = useRef<string | null>(null);
	useEffect(() => {
		const intent = vpn.connectIntent;
		if (!intent || !vpn.ready || profiles.loading || handledIntent.current === intent.id) return;
		handledIntent.current = intent.id;
		void vpn.clearConnectIntent();
		const profile = profiles.profiles.find((candidate) => candidate.id === intent.profileId);
		if (!profile) {
			toast.show({ tone: "error", title: "That connection no longer exists" });
			return;
		}
		if (vpn.state !== "disconnected") {
			toast.show({ tone: "info", title: "Another connection is already active" });
			return;
		}
		// A prompt still on screen belongs to an attempt the user is walking
		// away from; its pending toast would otherwise never be resolved.
		cancelCredentials();
		start(profile);
	}, [
		vpn.connectIntent,
		vpn.clearConnectIntent,
		vpn.ready,
		vpn.state,
		profiles.loading,
		profiles.profiles,
		cancelCredentials,
		start,
		toast,
	]);

	// A failure the helper reports resolves the attempt toast in place.
	useEffect(() => {
		if (!vpn.error) return;
		const id = attempt.current;
		attempt.current = null;
		const options = {
			tone: "error" as const,
			title: "Connection failed",
			description: vpn.error.msg,
		};
		if (id) {
			toast.update(id, options);
		} else {
			toast.show(options);
		}
	}, [vpn.error, toast]);

	useEffect(() => {
		if (!vpn.ready) return;
		const before = previousState.current;
		previousState.current = vpn.state;
		if (!primed.current) {
			primed.current = true;
			return;
		}
		if (before === vpn.state) return;

		const id = attempt.current;
		if (vpn.state === "connected") {
			attempt.current = null;
			const options = {
				tone: "success" as const,
				title: "Connected",
				description: active?.name,
			};
			if (id) toast.update(id, options);
			else toast.show(options);
			void profiles.reload();
		}
		if (vpn.state === "disconnected" && before !== "disconnected") {
			attempt.current = null;
			if (id) {
				toast.update(id, { tone: "error", title: "The connection did not come up" });
			} else if (before === "connected" || before === "disconnecting") {
				toast.show({ tone: "info", title: "Disconnected" });
			}
			void profiles.reload();
		}
	}, [vpn.state, vpn.ready, active?.name, toast, profiles.reload]);

	const save = useCallback(
		async (draft: ProfileDraft, password?: string) => {
			const saved = await profiles.save(draft, password);
			setEditing(null);
			toast.show({
				tone: "success",
				title: draft.id ? "Connection updated" : "Connection created",
				description: saved.name,
			});
			return saved;
		},
		[profiles.save, toast],
	);

	const remove = useCallback(
		async (profile: Profile) => {
			await profiles.remove(profile.id);
			toast.show({ tone: "info", title: "Connection deleted", description: profile.name });
		},
		[profiles.remove, toast],
	);

	return (
		<main className="flex min-h-dvh flex-col bg-secondary text-primary">
			<AppHeader
				state={vpn.state}
				onCreate={vpn.state === "disconnected" ? () => setEditing(emptyProfile) : undefined}
			/>

			<div className="flex flex-1 flex-col">
				{vpn.state === "disconnected" ? (
					<ConnectionList
						profiles={profiles.profiles}
						loading={profiles.loading}
						onConnect={start}
						onCreate={() => setEditing(emptyProfile)}
						onEdit={setEditing}
						onDelete={(profile) => void remove(profile)}
					/>
				) : (
					<ActiveConnection
						state={vpn.state}
						name={active?.name ?? "VPN"}
						portal={active?.portal ?? ""}
						connection={vpn.connection}
						onDisconnect={() => void vpn.disconnect()}
					/>
				)}
			</div>

			{editing && (
				<ConnectionForm
					draft={editing}
					credentialsAvailable={profiles.credentialsAvailable}
					onCancel={() => setEditing(null)}
					onSave={save}
				/>
			)}

			{askingCredentials && (
				<CredentialsDialog
					profile={askingCredentials}
					credentialsAvailable={profiles.credentialsAvailable}
					onCancel={cancelCredentials}
					onSubmit={(credentials) => {
						const profile = askingCredentials;
						setAskingCredentials(null);
						void run(profile, credentials);
					}}
				/>
			)}

			{vpn.certificate && (
				<CertTrustDialog
					details={vpn.certificate.details}
					fingerprint={vpn.certificate.fingerprint}
					onCancel={() => void vpn.rejectCertificate()}
					onTrust={() => void vpn.trustCertificate()}
				/>
			)}

			{vpn.challenge && (
				<MfaDialog
					message={vpn.challenge.message}
					onCancel={() => void vpn.disconnect()}
					onSubmit={(code) => void vpn.submitOtp(code)}
				/>
			)}

			{vpn.gatewayChoice && (
				<GatewayDialog
					gateways={vpn.gatewayChoice}
					onCancel={() => void vpn.disconnect()}
					onSelect={(address) => void vpn.selectGateway(address)}
				/>
			)}
		</main>
	);
};
