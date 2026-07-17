import { useState, useEffect } from "react";
import { motion, AnimatePresence } from "framer-motion";
import type { StreamEntry } from "../store";

interface ReasoningBlockProps {
  entry: StreamEntry & { kind: "reasoning" };
}

export default function ReasoningBlock({ entry }: ReasoningBlockProps) {
  const [expanded, setExpanded] = useState(false);

  useEffect(() => {
    if (entry.text.length > 0 && !expanded) {
      setExpanded(true);
    }
  }, [entry.text.length, expanded]);

  return (
    <div className="px-4 py-1">
      <button
        type="button"
        onClick={() => setExpanded(!expanded)}
        className="flex items-center gap-2 text-xs font-medium text-fog hover:text-mist transition-colors"
      >
        <motion.span
          animate={{ rotate: expanded ? 90 : 0 }}
          transition={{ duration: 0.15 }}
        >
          ▸
        </motion.span>
        {expanded ? "Hide reasoning" : "Show reasoning"}
      </button>
      <AnimatePresence>
        {expanded && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration: 0.2, ease: "easeInOut" }}
            className="overflow-hidden"
          >
            <div className="mt-2 border-l-2 border-fog/30 pl-3 text-sm leading-relaxed text-fog italic">
              {entry.text}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
