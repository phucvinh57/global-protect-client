import { ChevronDown, Globe01, Plus, Settings01 } from "@untitledui/icons";
import { type FormEvent, useState } from "react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { FileField, type FileFilter } from "@/components/ui/file-field";
import { ModalActions, ModalShell } from "@/components/ui/modal";
import { Select } from "@/components/ui/select";
import { TextField } from "@/components/ui/text-field";
import type { Profile, ProfileDraft } from "@/hooks/use-profiles";

/** Values the portal accepts for `clientos`. */
const clientOsOptions = [
	{ id: "Linux", label: "Linux" },
	{ id: "Windows", label: "Windows" },
	{ id: "Mac", label: "Mac" },
];

/** Hints for the native picker only — "All files" stays available for the rest. */
const certificateFilters: FileFilter[] = [
	{ name: "Certificates", extensions: ["pem", "crt", "cer", "der", "p12", "pfx"] },
	{ name: "All files", extensions: ["*"] },
];
const keyFilters: FileFilter[] = [
	{ name: "Keys", extensions: ["pem", "key", "p12", "pfx"] },
	{ name: "All files", extensions: ["*"] },
];

type ConnectionFormProps = {
	draft: ProfileDraft | Profile;
	credentialsAvailable: boolean;
	onCancel: () => void;
	onSave: (profile: ProfileDraft, password?: string) => Promise<unknown>;
};

/** Text inputs keep "" for an unset optional value; the backend wants null. */
const orNull = (value: string) => (value.trim() === "" ? null : value.trim());

export const ConnectionForm = ({
	draft,
	credentialsAvailable,
	onCancel,
	onSave,
}: ConnectionFormProps) => {
	const editing = draft.id !== "";
	const [form, setForm] = useState({
		name: draft.name,
		portal: draft.portal,
		username: draft.username,
		password: "",
		remember: draft.remember && credentialsAvailable,
		gateway: draft.gateway ?? "",
		clientOs: draft.clientOs ?? "Linux",
		cafile: draft.cafile ?? "",
		clientCert: draft.clientCert ?? "",
		clientKey: draft.clientKey ?? "",
		script: draft.script ?? "",
		hip: draft.hip,
		requiresOtp: draft.requiresOtp,
	});
	const [advanced, setAdvanced] = useState(false);
	const [saving, setSaving] = useState(false);
	const [error, setError] = useState<string | null>(null);

	const update = <K extends keyof typeof form>(key: K, value: (typeof form)[K]) =>
		setForm((current) => ({ ...current, [key]: value }));

	const submit = async (event: FormEvent<HTMLFormElement>) => {
		event.preventDefault();
		setSaving(true);
		setError(null);
		try {
			await onSave(
				{
					id: draft.id,
					name: form.name.trim(),
					portal: form.portal.trim(),
					username: form.username.trim(),
					remember: form.remember,
					gateway: orNull(form.gateway),
					clientOs: form.clientOs,
					cafile: orNull(form.cafile),
					clientCert: orNull(form.clientCert),
					clientKey: orNull(form.clientKey),
					script: orNull(form.script),
					hip: form.hip,
					requiresOtp: form.requiresOtp,
					trustedFingerprint: draft.trustedFingerprint,
				},
				form.password || undefined,
			);
		} catch (reason) {
			setError(reason instanceof Error ? reason.message : String(reason));
			setSaving(false);
		}
	};

	return (
		<ModalShell
			size="md"
			icon={editing ? Settings01 : Plus}
			title={editing ? "Edit connection" : "New connection"}
			description={
				editing
					? "Changes apply the next time you connect."
					: "Point gp-client at your GlobalProtect portal."
			}
		>
			<form className="flex min-h-0 flex-1 flex-col" onSubmit={submit}>
				{/* Only the fields scroll — Advanced sits below the fold, and the
				    actions below must stay reachable without scrolling to them. */}
				<div className="flex max-h-100 min-h-0 flex-1 flex-col gap-3 overflow-y-auto">
					<TextField
						label="Name"
						value={form.name}
						onChange={(value) => update("name", value)}
						placeholder="Work VPN"
						hint="Optional — the portal host is used when left blank."
					/>
					<TextField
						isRequired
						label="Portal"
						type="url"
						value={form.portal}
						onChange={(value) => update("portal", value)}
						placeholder="https://vpn.example.com"
						icon={<Globe01 className="size-4" />}
					/>
					<TextField
						isRequired
						label="Username"
						autoComplete="username"
						value={form.username}
						onChange={(value) => update("username", value)}
					/>
					<TextField
						label="Password"
						type="password"
						autoComplete="current-password"
						value={form.password}
						onChange={(value) => update("password", value)}
						isDisabled={!form.remember}
						placeholder={
							editing && draft.remember ? "Leave blank to keep the saved password" : undefined
						}
					/>
					<Checkbox
						isSelected={form.remember}
						isDisabled={!credentialsAvailable}
						onChange={(selected) => update("remember", selected)}
						label="Remember me"
						hint={
							credentialsAvailable
								? "Stores the password in your desktop keyring so connecting is one click."
								: "No desktop secret store is available, so passwords cannot be saved."
						}
					/>
					<Checkbox
						isSelected={form.requiresOtp}
						onChange={(selected) => update("requiresOtp", selected)}
						label="Ask for a one-time passcode"
						hint="Prompts for a code on every connection, even with a saved password."
					/>

					{/* A real border, not a ring: a ring is a box-shadow, and box-shadows
					    are outside the scroll container's overflow area, so the bottom
					    and left edges get clipped away here. */}
					<div className="border border-secondary">
						<button
							type="button"
							className="flex w-full cursor-pointer items-center justify-between gap-2 px-2.5 py-2 text-xs font-medium text-secondary"
							onClick={() => setAdvanced((current) => !current)}
						>
							Advanced
							<ChevronDown
								className={`size-4 text-fg-quaternary transition-transform duration-150 ${advanced ? "rotate-180" : ""}`}
							/>
						</button>
						{advanced && (
							<div className="flex flex-col gap-3 border-t border-secondary p-2.5">
								<Select
									label="Report this client as"
									items={clientOsOptions}
									selectedKey={form.clientOs}
									onSelectionChange={(key) => update("clientOs", key)}
									hint="Portals that reject Linux clients with a 512 error usually accept Windows."
								/>
								<TextField
									label="Gateway"
									value={form.gateway}
									onChange={(value) => update("gateway", value)}
									placeholder="Chosen through the portal when blank"
								/>
								<FileField
									label="CA certificate file"
									value={form.cafile}
									onChange={(value) => update("cafile", value)}
									filters={certificateFilters}
								/>
								<FileField
									label="Client certificate file"
									value={form.clientCert}
									onChange={(value) => update("clientCert", value)}
									filters={certificateFilters}
								/>
								<FileField
									label="Client key file"
									value={form.clientKey}
									onChange={(value) => update("clientKey", value)}
									filters={keyFilters}
								/>
								<FileField
									label="vpnc-script"
									value={form.script}
									onChange={(value) => update("script", value)}
									placeholder="Found automatically when blank"
								/>
								<Checkbox
									isSelected={form.hip}
									onChange={(selected) => update("hip", selected)}
									label="Submit a HIP report"
									hint="Only when the gateway asks for one."
								/>
							</div>
						)}
					</div>
				</div>

				{error && <p className="mt-3 bg-error-primary p-2 text-xs text-error-primary">{error}</p>}
				<ModalActions>
					<Button variant="secondary" isDisabled={saving} onPress={onCancel}>
						Cancel
					</Button>
					<Button type="submit" isDisabled={saving}>
						{saving ? "Saving…" : editing ? "Save changes" : "Create connection"}
					</Button>
				</ModalActions>
			</form>
		</ModalShell>
	);
};
