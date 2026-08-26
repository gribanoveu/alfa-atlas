import { useCallback, useEffect, useRef, useState } from "react";

const DISMISS_AFTER_MS = 3000;

export type AppToast = {
  message: string;
  variant: "success" | "error";
  onClose: () => void;
};

/** The app's single transient notification slot.
 *
 * Two sources feed it, with different lifecycles. Errors arrive as a *value*
 * that stays set (`editor.error`, or a failed folder open) — dismissing one
 * means remembering which message was dismissed, or the same string would
 * pop straight back. Successes are *events*, so each carries an id and is
 * cleared only if it is still the one showing, which keeps a later toast
 * from being cancelled by an earlier one's timer.
 *
 * A success outranks an error: it always reports something the user just
 * did, while the error may be stale. */
export function useAppToasts(errorSource: string | null) {
  const [folderError, setFolderError] = useState<string | null>(null);
  const [dismissedMessage, setDismissedMessage] = useState<string | null>(null);
  const [successToast, setSuccessToast] = useState<{ id: number; message: string } | null>(null);
  const counter = useRef(0);

  const showSuccess = useCallback((message: string) => {
    counter.current += 1;
    setSuccessToast({ id: counter.current, message });
  }, []);

  const errorMessage = errorSource ?? folderError;
  const visibleError =
    errorMessage && errorMessage !== dismissedMessage ? errorMessage : null;

  useEffect(() => {
    if (!errorMessage) return;
    const timer = setTimeout(() => setDismissedMessage(errorMessage), DISMISS_AFTER_MS);
    return () => clearTimeout(timer);
  }, [errorMessage]);

  useEffect(() => {
    if (!successToast) return;
    const timer = setTimeout(() => {
      setSuccessToast((current) => (current?.id === successToast.id ? null : current));
    }, DISMISS_AFTER_MS);
    return () => clearTimeout(timer);
  }, [successToast]);

  const toast: AppToast | null = successToast
    ? {
        message: successToast.message,
        variant: "success",
        onClose: () => setSuccessToast(null),
      }
    : visibleError
      ? {
          message: visibleError,
          variant: "error",
          onClose: () => setDismissedMessage(errorMessage),
        }
      : null;

  return { toast, showSuccess, folderError, setFolderError };
}
