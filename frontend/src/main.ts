import "./styles.css";
import { registerMaterialSymbols } from "./icons.ts";
import "./components/dash-app.ts";

registerMaterialSymbols();

// Follow the system light/dark preference.
let dark = window.matchMedia("(prefers-color-scheme: dark)");
let applyScheme = () =>
  document.documentElement.classList.toggle("wa-dark", dark.matches);
dark.addEventListener("change", applyScheme);
applyScheme();
