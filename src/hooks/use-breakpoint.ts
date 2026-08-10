import { useEffect, useState } from "react";

const screens = {
	sm: "640px",
	md: "768px",
	lg: "1024px",
	xl: "1280px",
	"2xl": "1536px",
};

/** Checks whether a Tailwind CSS viewport size applies. */
export const useBreakpoint = (size: keyof typeof screens) => {
	const [matches, setMatches] = useState(
		() => window.matchMedia(`(min-width: ${screens[size]})`).matches,
	);

	useEffect(() => {
		const breakpoint = window.matchMedia(`(min-width: ${screens[size]})`);

		setMatches(breakpoint.matches);

		const handleChange = (event: MediaQueryListEvent) => setMatches(event.matches);

		breakpoint.addEventListener("change", handleChange);
		return () => breakpoint.removeEventListener("change", handleChange);
	}, [size]);

	return matches;
};
