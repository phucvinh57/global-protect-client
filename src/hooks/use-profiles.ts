import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";

export type Profile = {
	id: string;
	name: string;
	portal: string;
	username: string;
	remember: boolean;
	cafile: string | null;
	clientCert: string | null;
	clientKey: string | null;
	trustedFingerprint: string | null;
	gateway: string | null;
	clientOs: string | null;
	script: string | null;
	hip: boolean;
	/** Ask for a one-time passcode on every connection, saved password or not. */
	requiresOtp: boolean;
	/** Whether the keyring currently holds a password for this profile. */
	hasSavedPassword: boolean;
};

/** What a profile looks like before the backend has given it an id. */
export type ProfileDraft = Omit<Profile, "id" | "hasSavedPassword"> & { id: string };

type ProfilesResponse = { profiles: Profile[]; credentialsAvailable: boolean };

export const emptyProfile: ProfileDraft = {
	id: "",
	name: "",
	portal: "",
	username: "",
	remember: true,
	cafile: null,
	clientCert: null,
	clientKey: null,
	trustedFingerprint: null,
	gateway: null,
	clientOs: "Linux",
	script: null,
	hip: false,
	requiresOtp: false,
};

/** Owns the saved connection list and keeps it in step with the backend. */
export const useProfiles = () => {
	const [profiles, setProfiles] = useState<Profile[]>([]);
	const [credentialsAvailable, setCredentialsAvailable] = useState(true);
	const [loading, setLoading] = useState(true);

	const reload = useCallback(async () => {
		const response = await invoke<ProfilesResponse>("profiles_load");
		setProfiles(response.profiles);
		setCredentialsAvailable(response.credentialsAvailable);
		setLoading(false);
	}, []);

	useEffect(() => {
		void reload();
	}, [reload]);

	const save = useCallback(
		async (profile: ProfileDraft, password?: string) => {
			const saved = await invoke<Profile>("profile_save", { profile, password: password ?? null });
			await reload();
			return saved;
		},
		[reload],
	);

	const remove = useCallback(
		async (id: string) => {
			await invoke("profile_delete", { id });
			await reload();
		},
		[reload],
	);

	return { profiles, credentialsAvailable, loading, reload, save, remove };
};
