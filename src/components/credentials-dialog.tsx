import { Lock01 } from "@untitledui/icons";
import { type FormEvent, useState } from "react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { ModalActions, ModalShell } from "@/components/ui/modal";
import { TextField } from "@/components/ui/text-field";
import type { Profile } from "@/hooks/use-profiles";

type CredentialsDialogProps = {
	profile: Profile;
	credentialsAvailable: boolean;
	onCancel: () => void;
	onSubmit: (credentials: { password: string; otp: string; remember: boolean }) => void;
};

/**
 * Asked for once per connection when the keyring cannot answer everything. A
 * saved password reduces this to the one-time passcode, which is never stored;
 * a typed password goes straight to the helper and is only kept if "remember
 * me" is on.
 */
export const CredentialsDialog = ({
	profile,
	credentialsAvailable,
	onCancel,
	onSubmit,
}: CredentialsDialogProps) => {
	// A saved password stays in the keyring: the helper falls back to it when
	// the prompt sends none.
	const askPassword = !profile.hasSavedPassword;
	const [password, setPassword] = useState("");
	const [otp, setOtp] = useState("");
	const [remember, setRemember] = useState(profile.remember && credentialsAvailable);

	const submit = (event: FormEvent<HTMLFormElement>) => {
		event.preventDefault();
		onSubmit({ password, otp, remember });
	};

	return (
		<ModalShell
			icon={Lock01}
			title={askPassword ? `Sign in to ${profile.name}` : `Connect to ${profile.name}`}
			description={`${profile.username} at ${profile.portal}`}
		>
			<form className="flex flex-col gap-3" onSubmit={submit}>
				{/* The dialog exists only to take these values, so it takes focus. */}
				{askPassword && (
					<TextField
						autoFocus
						isRequired
						label="Password"
						type="password"
						autoComplete="current-password"
						value={password}
						onChange={setPassword}
					/>
				)}
				<TextField
					autoFocus={!askPassword}
					isRequired={profile.requiresOtp}
					label="One-time passcode"
					type="text"
					autoComplete="one-time-code"
					inputMode="numeric"
					value={otp}
					onChange={setOtp}
					hint={
						profile.requiresOtp
							? askPassword
								? "Sent with the password, as this portal expects."
								: "Your saved password is used with this code."
							: "Only if your portal expects it together with the password."
					}
				/>
				{askPassword && (
					<Checkbox
						isSelected={remember}
						isDisabled={!credentialsAvailable}
						onChange={setRemember}
						label="Remember me"
						hint={
							credentialsAvailable
								? "Keeps this password in your desktop keyring."
								: "No desktop secret store is available on this machine."
						}
					/>
				)}
				<ModalActions>
					<Button variant="secondary" onPress={onCancel}>
						Cancel
					</Button>
					<Button type="submit">Connect</Button>
				</ModalActions>
			</form>
		</ModalShell>
	);
};
