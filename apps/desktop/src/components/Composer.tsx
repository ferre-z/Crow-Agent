import { useState, useCallback } from "react";
import { useReducedMotion } from "framer-motion";

interface ComposerProps {
  onSubmit: (text: string) => void;
  onStop: () => void;
  isRunning: boolean;
}

export default function Composer({ onSubmit, onStop, isRunning }: ComposerProps) {
  const [text, setText] = useState("");
  const [focused, setFocused] = useState(false);
  const reducedMotion = useReducedMotion();

  const handleSubmit = useCallback(() => {
    const trimmed = text.trim();
    if (!trimmed || isRunning) return;
    onSubmit(trimmed);
    setText("");
  }, [text, isRunning, onSubmit]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        handleSubmit();
      }
    },
    [handleSubmit],
  );

  return (
    <div className="px-4 pb-4">
      <div
        className={`relative rounded-2xl transition-shadow duration-200 ${
          focused && !reducedMotion ? "flow-border" : ""
        }`}
      >
        <div className="clay-inset flex items-end gap-2 p-3">
          <textarea
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={handleKeyDown}
            onFocus={() => setFocused(true)}
            onBlur={() => setFocused(false)}
            placeholder="Ask Crow anything..."
            rows={1}
            className="flex-1 resize-none bg-transparent font-sans text-sm text-mist placeholder:text-fog/50 focus:outline-none"
            disabled={isRunning}
          />
          {isRunning ? (
            <button
              type="button"
              onClick={onStop}
              className="flex h-8 w-8 items-center justify-center rounded-xl bg-coral/20 text-coral transition-colors hover:bg-coral/30"
              aria-label="Stop"
            >
              <svg
                width="12"
                height="12"
                viewBox="0 0 12 12"
                fill="currentColor"
              >
                <rect x="1" y="1" width="10" height="10" rx="2" />
              </svg>
            </button>
          ) : (
            <button
              type="button"
              onClick={handleSubmit}
              disabled={!text.trim()}
              className="flex h-8 w-8 items-center justify-center rounded-xl bg-sheen/20 text-sheen transition-colors hover:bg-sheen/30 disabled:opacity-30"
              aria-label="Send"
            >
              <svg
                width="14"
                height="14"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
              >
                <line x1="22" y1="2" x2="11" y2="13" />
                <polygon points="22 2 15 22 11 13 2 9 22 2" />
              </svg>
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
