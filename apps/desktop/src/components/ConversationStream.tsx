import { useRef, useEffect, useCallback, useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { useCrowStore } from "../store";
import UserBubble from "./UserBubble";
import AssistantText from "./AssistantText";
import ReasoningBlock from "./ReasoningBlock";
import ToolCard from "./ToolCard";
import RunBanner from "./RunBanner";

export default function ConversationStream() {
  const entries = useCrowStore((s) => s.entries);
  const activeSessionId = useCrowStore((s) => s.activeSessionId);
  const setSelectedEntry = useCrowStore((s) => s.setSelectedEntry);
  const bottomRef = useRef<HTMLDivElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const [scrimVisible, setScrimVisible] = useState(false);

  useEffect(() => {
    setScrimVisible(true);
    const t = setTimeout(() => setScrimVisible(false), 200);
    return () => clearTimeout(t);
  }, [activeSessionId]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const isNearBottom =
      container.scrollHeight - container.scrollTop - container.clientHeight < 80;
    if (isNearBottom) {
      bottomRef.current?.scrollIntoView({ behavior: "smooth" });
    }
  }, [entries]);

  const handleEntryClick = useCallback(
    (index: number) => {
      setSelectedEntry(index);
    },
    [setSelectedEntry],
  );

  return (
    <div ref={containerRef} className="relative flex-1 overflow-y-auto py-4">
      <AnimatePresence>
        {scrimVisible && (
          <motion.div
            initial={{ opacity: 0.4 }}
            animate={{ opacity: 0 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.2 }}
            className="pointer-events-none absolute inset-0 z-10 bg-obsidian/60"
          />
        )}
      </AnimatePresence>

      <AnimatePresence initial={false}>
        {entries.map((entry, i) => {
          const key =
            entry.kind === "tool"
              ? `tool-${entry.call_id}`
              : entry.kind === "user" || entry.kind === "assistant"
                ? `${entry.kind}-${entry.id}`
                : `${entry.kind}-${i}`;

          const clickHandler = () => handleEntryClick(i);

          switch (entry.kind) {
            case "user":
              return (
                <div key={key} onClick={clickHandler} className="cursor-pointer">
                  <UserBubble entry={entry} />
                </div>
              );
            case "assistant":
              return (
                <div key={key} onClick={clickHandler} className="cursor-pointer">
                  <AssistantText entry={entry} />
                </div>
              );
            case "reasoning":
              return (
                <div key={key} onClick={clickHandler} className="cursor-pointer">
                  <ReasoningBlock entry={entry} />
                </div>
              );
            case "tool":
              return (
                <div key={key} onClick={clickHandler} className="cursor-pointer">
                  <ToolCard entry={entry} />
                </div>
              );
            case "run_finished":
            case "run_failed":
            case "run_cancelled":
              return <RunBanner key={key} entry={entry} />;
            default:
              return null;
          }
        })}
      </AnimatePresence>
      <div ref={bottomRef} />
    </div>
  );
}
