// In-app themed dialogs replacing the OS-native plugin-dialog pop-ups.
// One request at a time; DialogHost in the layout renders the active one.

export interface DialogButton {
  label: string;
  value: string;
  kind: "gold" | "ghost" | "danger";
}

export interface DialogRequest {
  title: string;
  message: string;
  buttons: DialogButton[];
}

interface DialogState {
  current: DialogRequest | null;
  resolve: ((value: string | null) => void) | null;
}

export const dialog = $state<DialogState>({ current: null, resolve: null });

/** Show a dialog; resolves with the chosen button's value, or null on cancel/backdrop/Esc. */
export function askDialog(req: DialogRequest): Promise<string | null> {
  // a second request while one is open cancels the first (should not happen in practice)
  dialog.resolve?.(null);
  return new Promise((resolve) => {
    dialog.current = req;
    dialog.resolve = resolve;
  });
}

export function settleDialog(value: string | null) {
  const r = dialog.resolve;
  dialog.current = null;
  dialog.resolve = null;
  r?.(value);
}

/** Confirmation gate. `danger: true` styles the confirm button red. */
export async function appConfirm(opts: {
  title: string;
  message: string;
  confirmLabel?: string;
  danger?: boolean;
}): Promise<boolean> {
  const v = await askDialog({
    title: opts.title,
    message: opts.message,
    buttons: [
      { label: opts.confirmLabel ?? "Confirm", value: "ok", kind: opts.danger ? "danger" : "gold" },
      { label: "Cancel", value: "cancel", kind: "ghost" },
    ],
  });
  return v === "ok";
}
