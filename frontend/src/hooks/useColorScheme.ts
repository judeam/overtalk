"use client";
import { useEffect, useState } from "react";
/** Match the app's root-class overrides and OS preference, including live changes. */
export function useColorScheme(): "light" | "dark" {
  const [scheme, setScheme] = useState<"light" | "dark">("light");
  useEffect(() => {
    const root = document.documentElement;
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const update = () =>
      setScheme(
        root.classList.contains("dark") ||
          (!root.classList.contains("light") && media.matches)
          ? "dark"
          : "light",
      );
    const observer = new MutationObserver(update);
    observer.observe(root, { attributes: true, attributeFilter: ["class"] });
    media.addEventListener("change", update);
    update();
    return () => {
      observer.disconnect();
      media.removeEventListener("change", update);
    };
  }, []);
  return scheme;
}
