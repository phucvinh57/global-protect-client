import { DotsVertical, Edit01, Lock01, Plus, Shield01, Trash01 } from "@untitledui/icons";
import { useState } from "react";
import { Button as AriaButton, Menu, MenuItem, MenuTrigger, Popover } from "react-aria-components";
import { Button } from "@/components/ui/button";
import { ModalActions, ModalShell } from "@/components/ui/modal";
import type { Profile } from "@/hooks/use-profiles";

type ConnectionListProps = {
	profiles: Profile[];
	loading: boolean;
	onConnect: (profile: Profile) => void;
	onCreate: () => void;
	onEdit: (profile: Profile) => void;
	onDelete: (profile: Profile) => void;
};

export const ConnectionList = ({
	profiles,
	loading,
	onConnect,
	onCreate,
	onEdit,
	onDelete,
}: ConnectionListProps) => {
	const [pendingDelete, setPendingDelete] = useState<Profile | null>(null);

	if (loading) {
		return (
			<div className="flex flex-col divide-y divide-secondary border-b border-secondary">
				{[0, 1].map((row) => (
					<div key={row} className="h-12 animate-pulse bg-secondary" />
				))}
			</div>
		);
	}

	if (profiles.length === 0) {
		return <EmptyState onCreate={onCreate} />;
	}

	return (
		<>
			<ul className="flex flex-col divide-y divide-secondary border-b border-secondary">
				{profiles.map((profile) => (
					<li key={profile.id}>
						<ConnectionRow
							profile={profile}
							onConnect={() => onConnect(profile)}
							onEdit={() => onEdit(profile)}
							onDelete={() => setPendingDelete(profile)}
						/>
					</li>
				))}
			</ul>

			{pendingDelete && (
				<ModalShell
					icon={Trash01}
					tone="error"
					title={`Delete “${pendingDelete.name}”?`}
					description="The saved password is removed from the keyring too. This cannot be undone."
				>
					<ModalActions>
						<Button variant="secondary" onPress={() => setPendingDelete(null)}>
							Cancel
						</Button>
						<Button
							variant="destructive"
							onPress={() => {
								onDelete(pendingDelete);
								setPendingDelete(null);
							}}
						>
							Delete
						</Button>
					</ModalActions>
				</ModalShell>
			)}
		</>
	);
};

type ConnectionRowProps = {
	profile: Profile;
	onConnect: () => void;
	onEdit: () => void;
	onDelete: () => void;
};

/**
 * The whole row connects — there is nothing else it could do. The overlay
 * button keeps that target one element while leaving the row's own menu
 * button clickable above it.
 */
const ConnectionRow = ({ profile, onConnect, onEdit, onDelete }: ConnectionRowProps) => (
	<div className="relative flex items-center gap-2 bg-primary px-3 py-2 transition duration-100 ease-linear hover:bg-secondary">
		<AriaButton
			aria-label={`Connect to ${profile.name}`}
			className="absolute inset-0 z-0 cursor-pointer outline-focus-ring focus-visible:outline-2 focus-visible:-outline-offset-2"
			onPress={onConnect}
		/>
		<div className="pointer-events-none min-w-0 flex-1">
			<div className="flex items-baseline gap-2">
				<p className="truncate text-sm font-semibold text-primary">{profile.name}</p>
				<p className="truncate text-xs text-quaternary">{profile.username}</p>
				{profile.hasSavedPassword && (
					<Lock01 className="size-3 shrink-0 translate-y-0.5 text-fg-success-primary" />
				)}
			</div>
			<p className="truncate text-xs text-tertiary">{profile.portal}</p>
		</div>
		<MenuTrigger>
			<AriaButton
				aria-label={`Options for ${profile.name}`}
				className="relative z-10 -mr-1 cursor-pointer p-1 text-fg-quaternary transition hover:bg-tertiary hover:text-fg-secondary pressed:bg-tertiary"
			>
				<DotsVertical className="size-4" />
			</AriaButton>
			<Popover
				offset={4}
				placement="bottom end"
				className="min-w-40 bg-primary py-1 shadow-lg ring-1 ring-secondary_alt entering:animate-in entering:fade-in entering:zoom-in-95 entering:duration-100 exiting:animate-out exiting:fade-out exiting:duration-75"
			>
				<Menu className="outline-hidden">
					<MenuItem
						className="flex cursor-pointer items-center gap-2 px-3 py-1.5 text-sm text-secondary outline-hidden hover:bg-primary_hover focus:bg-primary_hover"
						onAction={onEdit}
					>
						<Edit01 className="size-4" />
						Edit connection
					</MenuItem>
					<MenuItem
						className="flex cursor-pointer items-center gap-2 px-3 py-1.5 text-sm text-error-primary outline-hidden hover:bg-error-primary focus:bg-error-primary"
						onAction={onDelete}
					>
						<Trash01 className="size-4" />
						Delete
					</MenuItem>
				</Menu>
			</Popover>
		</MenuTrigger>
	</div>
);

const EmptyState = ({ onCreate }: { onCreate: () => void }) => (
	<div className="flex flex-1 flex-col items-center justify-center px-8 py-10 text-center">
		<span className="flex size-10 items-center justify-center bg-secondary text-fg-quaternary ring-1 ring-secondary">
			<Shield01 className="size-5" />
		</span>
		<h2 className="mt-3 text-sm font-semibold text-primary">No connections yet</h2>
		<p className="mt-1 max-w-xs text-xs text-tertiary">
			Add the GlobalProtect portal you sign in to and it will be waiting here next time.
		</p>
		<Button className="mt-4" size="sm" iconLeading={Plus} onPress={onCreate}>
			Create connection
		</Button>
	</div>
);
