import { useState, useEffect } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { useCrowStore, type StreamEntry } from "../store";

function formatArgs(args: unknown): string {
  if (args == null) return "";
  return JSON.stringify(args, null, 2);
}

function toolEntryDetail(entry: StreamEntry & { kind: "tool" }) {
  return (
    <div className="space-y-4">
      <div>
        <div className="mb-1 font-display text-xs font-medium uppercase tracking-wider text-fog">
          Tool
        </div>
        <div className="font-mono text-sm text-mist">{entry.name}</div>
      </div>

      <div>
        <div className="mb-1 font-display text-xs font-medium uppercase tracking-wider text-fog">
          Arguments
        </div>
        <div className="clay-inset max-h-48 overflow-auto rounded-lg">
          <pre className="p-3 font-mono text-xs text-mist whitespace-pre-wrap">
            {formatArgs(entry.args)}
          </pre>
        </div>
      </div>

      <div>
        <div className="mb-1 font-display text-xs font-medium uppercase tracking-wider text-fog">
          Status
        </div>
        <div
          className={`text-sm font-medium ${
            entry.status === "running"
              ? "text-sheen"
              : entry.status === "success"
                ? "text-halo"
                : "text-coral"
          }`}
        >
          {entry.status}
        </div>
      </div>

      {entry.outputChunks.length > 0 && (
        <div>
          <div className="mb-1 font-display text-xs font-medium uppercase tracking-wider text-fog">
            Output
          </div>
          <div className="clay-inset max-h-64 overflow-auto rounded-lg">
            <pre className="p-3 font-mono text-xs text-mist whitespace-pre-wrap">
              {entry.outputChunks.join("")}
            </pre>
          </div>
        </div>
      )}
    </div>
  );
}

function userEntryDetail(entry: StreamEntry & { kind: "user" }) {
  return (
    <div>
      <div className="mb-1 font-display text-xs font-medium uppercase tracking-wider text-fog">
        User Message
      </div>
      <div className="text-sm text-mist">{entry.content}</div>
    </div>
  );
}

function assistantEntryDetail(entry: StreamEntry & { kind: "assistant" }) {
  return (
    <div>
      <div className="mb-1 font-display text-xs font-medium uppercase tracking-wider text-fog">
        Assistant
      </div>
      <div className="text-sm text-mist whitespace-pre-wrap">{entry.text}</div>
    </div>
  );
}

function useIsNarrow(breakpoint: number): boolean {
  const [narrow, setNarrow] = useState(
    () => window.innerWidth < breakpoint,
  );
  useEffect(() => {
    const mq = window.matchMedia(`(max-width: ${breakpoint - 1}px)`);
    const handler = (e: MediaQueryListEvent) => setNarrow(e.matches);
    mq.addEventListener("change", handler);
    setNarrow(mq.matches);
    return () => mq.removeEventListener("change", handler);
  }, [breakpoint]);
  return narrow;
}

export default function Inspector() {
  const entries = useCrowStore((s) => s.entries);
  const selectedIndex = useCrowStore((s) => s.selectedEntryIndex);
  const setSelectedEntry = useCrowStore((s) => s.setSelectedEntry);
  const isSheet = useIsNarrow(1100);

  const [sheetOpen, setSheetOpen] = useState(false);

  useEffect(() => {
    if (selectedIndex !== null) setSheetOpen(true);
  }, [selectedIndex]);

  if (selectedIndex === null || selectedIndex >= entries.length) {
    if (isSheet) return null;
    return (
      <div className="clay flex w-72 flex-col items-center justify-center p-6">
        <div className="text-center text-xs text-fog">
          Select an item to inspect
        </div>
      </div>
    );
  }

  const entry = entries[selectedIndex];

  let detail: React.ReactNode = null;
  if (entry.kind === "tool") {
    detail = toolEntryDetail(entry);
  } else if (entry.kind === "user") {
    detail = userEntryDetail(entry);
  } else if (entry.kind === "assistant") {
    detail = assistantEntryDetail(entry);
  } else {
    detail = (
      <div className="text-xs text-fog">No detail available for this item.</div>
    );
  }

  if (isSheet) {
    return (
      <AnimatePresence>
        {sheetOpen && (
          <>
            <motion.div
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              className="fixed inset-0 z-30 bg-black/30"
              onClick={() => {
                setSheetOpen(false);
                setSelectedEntry(null);
              }}
            />
            <motion.div
              initial={{ y: "100%" }}
              animate={{ y: 0 }}
              exit={{ y: "100%" }}
              transition={{ type: "spring", stiffness: 300, damping: 30 }}
              className="inspector-sheet clay p-4"
            >
              <div className="mb-3 flex items-center justify-between">
                <span className="font-display text-xs font-medium uppercase tracking-wider text-fog">
                  Inspector
                </span>
                <button
                  type="button"
                  onClick={() => {
                    setSheetOpen(false);
                    setSelectedEntry(null);
                  }}
                  className="text-fog text-xs hover:text-mist"
                >
                  Close
                </button>
              </div>
              <div className="overflow-y-auto">{detail}</div>
            </motion.div>
          </>
        )}
      </AnimatePresence>
    );
  }

  return (
    <div className="clay flex w-72 flex-col overflow-hidden">
      <div className="flex items-center justify-between px-4 py-3">
        <span className="font-display text-xs font-medium uppercase tracking-wider text-fog">
          Inspector
        </span>
      </div>
      <div className="flex-1 overflow-y-auto px-4 pb-4">{detail}</div>
    </div>
  );
}
