/** Shows a short message at the bottom of the window. Any command can call this. */
export function showToast(message: string) {
  window.dispatchEvent(new CustomEvent("spark:toast", { detail: message }));
}

/** Commands that confirm themselves with a fixed message. Toggles with a state call `showToast` directly. */
export const COMMAND_TOASTS: Record<string, string> = {
  zoomFit: "Zoom to fit",
  zoomSelection: "Zoom to selection",
  zoom100: "Zoom to 100%",
};
