import { AlertTriangle } from "@untitledui/icons";
import { Button } from "@/components/ui/button";
import { ModalActions, ModalShell } from "@/components/ui/modal";

type CertTrustDialogProps = {
	details: string;
	fingerprint: string;
	onCancel: () => void;
	onTrust: () => void;
};

export const CertTrustDialog = ({
	details,
	fingerprint,
	onCancel,
	onTrust,
}: CertTrustDialogProps) => (
	<ModalShell
		size="md"
		icon={AlertTriangle}
		tone="warning"
		title="Untrusted VPN certificate"
		description="The portal certificate could not be validated. Only continue if you recognise the fingerprint below."
	>
		<pre className="max-h-36 overflow-auto bg-error-primary p-2 font-mono text-xs whitespace-pre-wrap text-error-primary">
			{details}
		</pre>
		<p className="mt-2 bg-secondary p-2 font-mono text-xs break-all text-secondary">
			{fingerprint}
		</p>
		<ModalActions>
			<Button variant="secondary" onPress={onCancel}>
				Cancel
			</Button>
			<Button variant="destructive" onPress={onTrust}>
				Trust certificate
			</Button>
		</ModalActions>
	</ModalShell>
);
