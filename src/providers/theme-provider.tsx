import type { ReactNode } from "react";
import { createContext, useContext, useEffect, useState } from "react";

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
	storageKey?: string;
}

export const ThemeProvider = ({
	children,
	defaultTheme = "system",
	storageKey = "ui-theme",
	darkModeClass = "dark-mode",
}: ThemeProviderProps) => {
	const [theme, setTheme] = useState<Theme>(() => {
		const savedTheme = localStorage.getItem(storageKey) as Theme | null;
		return savedTheme || defaultTheme;
	});

	useEffect(() => {
		const applyTheme = () => {
			const root = document.documentElement;

			if (theme === "system") {
				const systemTheme = window.matchMedia("(prefers-color-scheme: dark)").matches
					? "dark"
					: "light";

				root.classList.toggle(darkModeClass, systemTheme === "dark");
				localStorage.removeItem(storageKey);
			} else {
				root.classList.toggle(darkModeClass, theme === "dark");
				localStorage.setItem(storageKey, theme);
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
	}, [darkModeClass, storageKey, theme]);

	return <ThemeContext.Provider value={{ theme, setTheme }}>{children}</ThemeContext.Provider>;
};
