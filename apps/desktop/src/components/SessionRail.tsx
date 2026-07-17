import { useCallback } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { useCrowStore } from "../store";
import * as client from "../ipc/client";

export default function SessionRail() {
  const sessions = useCrowStore((s) => s.sessions);
  const activeSessionId = useCrowStore((s) => s.activeSessionId);
  const setActiveSession = useCrowStore((s) => s.setActiveSession);
  const applyReplayEvents = useCrowStore((s) => s.applyReplayEvents);

  const handleSelect = useCallback(
    async (sessionId: string, path: string) => {
      if (sessionId === activeSessionId) return;
      setActiveSession(sessionId, path);
      try {
        const result = await client.sessionLoad(sessionId, path);
        applyReplayEvents(result.events);
      } catch (err) {
        console.error("session_load failed:", err);
      }
    },
    [activeSessionId, setActiveSession, applyReplayEvents],
  );

  return (
    <div className="clay flex h-full w-60 flex-col overflow-hidden">
      <div className="flex items-center justify-between px-4 py-3">
        <span className="font-display text-xs font-medium uppercase tracking-wider text-fog">
          Sessions
        </span>
      </div>

      <div className="flex-1 overflow-y-auto px-2 pb-2">
        <AnimatePresence initial={false}>
          {sessions.map((session) => {
            const isActive = session.session_id === activeSessionId;
            const date = new Date(session.started_at).toLocaleDateString(
              undefined,
              { month: "short", day: "numeric" },
            );
            const shortId = session.session_id.slice(0, 8);

            return (
              <motion.button
                key={session.session_id}
                initial={{ opacity: 0, x: -8 }}
                animate={{ opacity: 1, x: 0 }}
                exit={{ opacity: 0, x: -8 }}
                onClick={() => handleSelect(session.session_id, session.path)}
                className={`mb-1 flex w-full items-center gap-2 rounded-xl px-3 py-2 text-left text-sm transition-colors ${
                  isActive
                    ? "bg-white/5 text-mist"
                    : "text-fog hover:bg-white/[0.03] hover:text-mist"
                }`}
              >
                <span className="font-mono text-[10px] text-fog">
                  {shortId}
                </span>
                <span className="ml-auto text-[10px] text-fog">{date}</span>
              </motion.button>
            );
          })}
        </AnimatePresence>

        {sessions.length === 0 && (
          <div className="px-3 py-6 text-center text-xs text-fog">
            No sessions yet
          </div>
        )}
      </div>
    </div>
  );
}
