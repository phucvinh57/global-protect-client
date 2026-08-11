import { invoke } from "@tauri-apps/api/core";
import type { ReactNode } from "react";
import {
	createContext,
	useCallback,
	useContext,
	useEffect,
	useLayoutEffect,
	useState,
} from "react";

type Theme = "light" | "dark" | "system";

interface ThemeContextType {
	theme: Theme;
	setTheme: (theme: Theme) => void;
}

const ThemeContext = createContext<ThemeContextType | undefined>(undefined);

export const useTheme = (): ThemeContextType => {
	const context = useContext(ThemeContext);

	if (context === undefined) {
		throw new Error("useTheme must be used within a ThemeProvider");
	}

	return context;
};

interface ThemeProviderProps {
	children: ReactNode;
	darkModeClass?: string;
	defaultTheme?: Theme;
}

export const ThemeProvider = ({
	children,
	defaultTheme = "system",
	darkModeClass = "dark-mode",
}: ThemeProviderProps) => {
	const [theme, setCurrentTheme] = useState<Theme>(defaultTheme);
	const [ready, setReady] = useState(false);

	useEffect(() => {
		let mounted = true;
		void invoke<{ theme: Theme }>("preferences_load")
			.then((preferences) => {
				if (mounted) setCurrentTheme(preferences.theme);
			})
			.catch(() => undefined)
			.finally(() => {
				if (mounted) setReady(true);
			});
		return () => {
			mounted = false;
		};
	}, []);

	useLayoutEffect(() => {
		const applyTheme = () => {
			const root = document.documentElement;

			if (theme === "system") {
				const systemTheme = window.matchMedia("(prefers-color-scheme: dark)").matches
					? "dark"
					: "light";

				root.classList.toggle(darkModeClass, systemTheme === "dark");
			} else {
				root.classList.toggle(darkModeClass, theme === "dark");
			}
		};

		applyTheme();

		const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
		const handleChange = () => {
			if (theme === "system") {
				applyTheme();
			}
		};

		mediaQuery.addEventListener("change", handleChange);
		return () => mediaQuery.removeEventListener("change", handleChange);
	}, [darkModeClass, theme]);

	const setTheme = useCallback((next: Theme) => {
		setCurrentTheme(next);
		void invoke("theme_set", { theme: next }).catch(() => undefined);
	}, []);

	if (!ready) return null;
	return <ThemeContext.Provider value={{ theme, setTheme }}>{children}</ThemeContext.Provider>;
};
