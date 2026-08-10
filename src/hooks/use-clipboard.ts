import { useCallback, useState } from "react";

const DEFAULT_TIMEOUT = 2000;

type CopyResult = { success: boolean; error?: Error };

type UseClipboardReturnType = {
	copied: string | boolean;
	copy: (text: string, id?: string) => Promise<CopyResult>;
};

/** Copies text to the clipboard and briefly exposes the copied state. */
export const useClipboard = (): UseClipboardReturnType => {
	const [copied, setCopied] = useState<string | boolean>(false);

	const fallback = useCallback((text: string, id?: string): CopyResult => {
		try {
			const textArea = document.createElement("textarea");
			textArea.value = text;
			textArea.style.position = "absolute";
			textArea.style.left = "-99999px";

			document.body.appendChild(textArea);
			textArea.select();

			const success = document.execCommand("copy");
			textArea.remove();

			setCopied(id || true);
			setTimeout(() => setCopied(false), DEFAULT_TIMEOUT);

			return success
				? { success: true }
				: { success: false, error: new Error("execCommand returned false") };
		} catch (error) {
			return {
				success: false,
				error: error instanceof Error ? error : new Error("Fallback copy failed"),
			};
		}
	}, []);

	const copy = useCallback(
		async (text: string, id?: string) => {
			if (navigator.clipboard && window.isSecureContext) {
				try {
					await navigator.clipboard.writeText(text);

					setCopied(id || true);
					setTimeout(() => setCopied(false), DEFAULT_TIMEOUT);

					return { success: true };
				} catch {
					return fallback(text, id);
				}
			}

			return fallback(text, id);
		},
		[fallback],
	);

	return { copied, copy };
};
