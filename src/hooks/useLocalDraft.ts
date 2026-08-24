/**
 * Keeps a local copy of a field while the user types, and reports when it differs from
 * what is stored.
 *
 * The report editor saves explicitly rather than on every keystroke, so the screen needs
 * to know which answers are dirty. `reset` is called after a successful save.
 */

import { useCallback, useEffect, useRef, useState } from "react";

export function useLocalDraft<T>(stored: T) {
  const [value, setValue] = useState<T>(stored);
  const storedRef = useRef(stored);

  useEffect(() => {
    // Adopt a new stored value only when it really changed, so a re-render caused by an
    // unrelated update does not discard what the user is typing.
    if (!Object.is(storedRef.current, stored)) {
      storedRef.current = stored;
      setValue(stored);
    }
  }, [stored]);

  const reset = useCallback((next: T) => {
    storedRef.current = next;
    setValue(next);
  }, []);

  return { value, setValue, reset, dirty: !Object.is(value, storedRef.current) };
}
