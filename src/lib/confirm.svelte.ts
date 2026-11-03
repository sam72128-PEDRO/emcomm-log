export interface ConfirmButton {
  id: string;
  label: string;
  tone?: "primary" | "danger" | "default";
}

export interface ConfirmRequest {
  title: string;
  message: string;
  detail?: string;
  buttons: ConfirmButton[];
  /** Optional free-text input (e.g. a void reason). */
  input?: { label: string; value: string; maxLength?: number };
  /** Button id returned when the dialog is dismissed with Esc. */
  cancelId: string;
}

interface Pending extends ConfirmRequest {
  resolve: (r: { id: string; value: string }) => void;
}

class ConfirmStore {
  current = $state<Pending | null>(null);

  ask(req: ConfirmRequest): Promise<{ id: string; value: string }> {
    return new Promise((resolve) => {
      this.current = { ...req, input: req.input ? { ...req.input } : undefined, resolve };
    });
  }

  answer(id: string, value = "") {
    const c = this.current;
    this.current = null;
    c?.resolve({ id, value });
  }
}

export const confirm = new ConfirmStore();
