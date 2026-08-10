import { Key01 } from "@untitledui/icons";
import { type FormEvent, useState } from "react";
import { Button } from "@/components/ui/button";
import { ModalActions, ModalShell } from "@/components/ui/modal";
import { TextField } from "@/components/ui/text-field";

type MfaDialogProps = {
	message: string;
	onCancel: () => void;
	onSubmit: (code: string) => void;
};

/**
 * Answers a gateway challenge. The code goes straight to the helper rather than
 * into a saved profile, so it is never stored or reused.
 */
export const MfaDialog = ({ message, onCancel, onSubmit }: MfaDialogProps) => {
	const [code, setCode] = useState("");

	return (
		<ModalShell icon={Key01} title="Additional authentication" description={message}>
			<form
				className="flex flex-col gap-3"
				onSubmit={(event: FormEvent<HTMLFormElement>) => {
					event.preventDefault();
					onSubmit(code);
				}}
			>
				{/* The dialog exists only to take this value, so it takes focus. */}
				<TextField
					autoFocus
					isRequired
					label="Code"
					type="password"
					autoComplete="one-time-code"
					inputMode="numeric"
					value={code}
					onChange={setCode}
				/>
				<ModalActions>
					<Button variant="secondary" onPress={onCancel}>
						Cancel
					</Button>
					<Button type="submit">Submit</Button>
				</ModalActions>
			</form>
		</ModalShell>
	);
};
