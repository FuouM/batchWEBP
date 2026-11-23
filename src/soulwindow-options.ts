/**
 * SoulWindow options, shared by the Vite plugin (pre-paint boot script) and the runtime
 * controller (src/theme.ts). Pure data: vite.config.ts loads this file in Node.
 */
import type { SoulWindowOptions } from "soulwindow/runtime";

export const SOULWINDOW_OPTIONS = {
  storagePrefix: "batchwebp-",
  metaId: "batchwebp-theme-color-meta",
  defaultTheme: "dark",
  density: "compact",
} as const satisfies SoulWindowOptions;
