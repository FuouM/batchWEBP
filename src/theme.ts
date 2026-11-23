/** Theme controller: SoulWindow owns persistence and the data-theme / data-density attributes. */
import { createRoot } from "solid-js";
import { createSoulWindow } from "soulwindow/runtime";
import { useSoulWindow } from "soulwindow/solid/store";
import { SOULWINDOW_OPTIONS } from "./soulwindow-options";

export const sw = createSoulWindow(SOULWINDOW_OPTIONS);

// App-lifetime subscription: the root is never disposed.
export const swSignals = createRoot(() => useSoulWindow(sw));
