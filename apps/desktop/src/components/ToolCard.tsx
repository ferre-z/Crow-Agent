import { useState, useEffect } from "react";
import { motion, AnimatePresence } from "framer-motion";
import type { StreamEntry } from "../store";

interface ToolCardProps {
  entry: StreamEntry & { kind: "tool" };
}

const TOOL_ICONS: Record<string, string> = {
  read: "📄",
  write: "✏️",
  edit: "✏️",
  bash: "⬛",
  shell: "⬛",
};

export default function ToolCard({ entry }: ToolCardProps) {
  const [expanded, setExpanded] = useState(entry.status === "running");
  const isRunning = entry.status === "running";
  const output = entry.outputChunks.join("");
  const icon = TOOL_ICONS[entry.name] ?? "🔧";

  useEffect(() => {
    if (isRunning) setExpanded(true);
  }, [isRunning]);

  return (
    <motion.div
      layout
      initial={{ opacity: 0, y: 12 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.25 }}
      className="px-4 py-1"
    >
      <div className="relative rounded-2xl p-[2px]">
        {isRunning && (
          <div
            className="absolute inset-0 rounded-2xl"
            style={{
              background:
                "conic-gradient(from var(--angle), var(--sheen), var(--iris), var(--halo), var(--sheen))",
              animation: "rotate-angle 3s linear infinite",
            }}
          />
        )}
        {!isRunning && (
          <div className="absolute inset-0 rounded-2xl bg-white/5" />
        )}

        <div className="relative clay-card overflow-hidden rounded-[22px]">
          <button
            type="button"
            onClick={() => setExpanded(!expanded)}
            className="flex w-full items-center gap-3 px-4 py-3 text-left"
          >
            <span className="text-base">{icon}</span>
            <span className="font-display text-sm font-medium text-mist flex-1">
              {entry.name}
            </span>
            <motion.span
              animate={{ rotate: expanded ? 180 : 0 }}
              transition={{ duration: 0.15 }}
              className="text-fog text-xs"
            >
              ▾
            </motion.span>
          </button>

          <AnimatePresence>
            {expanded && (
              <motion.div
                initial={{ height: 0 }}
                animate={{ height: "auto" }}
                exit={{ height: 0 }}
                transition={{ duration: 0.25, ease: "easeInOut" }}
                className="overflow-hidden"
              >
                {entry.args != null && (
                  <div className="border-t border-white/5 px-4 py-2">
                    <div className="font-mono text-xs text-fog">
                      {JSON.stringify(entry.args, null, 2)}
                    </div>
                  </div>
                )}
                {output && (
                  <div className="clay-inset mx-3 mb-3 max-h-64 overflow-auto rounded-lg">
                    <pre className="p-3 font-mono text-xs leading-relaxed text-mist whitespace-pre-wrap">
                      {output}
                    </pre>
                  </div>
                )}
                {isRunning && !output && (
                  <div className="px-4 pb-3 text-xs text-fog italic">
                    Running...
                  </div>
                )}
              </motion.div>
            )}
          </AnimatePresence>
        </div>
      </div>
    </motion.div>
  );
}
