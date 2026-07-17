import { useCallback, useRef, useEffect, useState } from "react";
import { motion } from "framer-motion";
import { Channel } from "@tauri-apps/api/core";
import { useCrowStore } from "./store";
import type { AgentEvent, AskNotification } from "./ipc/events";
import * as client from "./ipc/client";
import TopBar from "./components/TopBar";
import SessionRail from "./components/SessionRail";
import ConversationStream from "./components/ConversationStream";
import Composer from "./components/Composer";
import Inspector from "./components/Inspector";
import ApprovalOverlay from "./components/ApprovalOverlay";

function useMediaQuery(query: string): boolean {
  const [matches, setMatches] = useState(
    () => window.matchMedia(query).matches,
  );
  useEffect(() => {
    const mq = window.matchMedia(query);
    const handler = (e: MediaQueryListEvent) => setMatches(e.matches);
    mq.addEventListener("change", handler);
    setMatches(mq.matches);
    return () => mq.removeEventListener("change", handler);
  }, [query]);
  return matches;
}

export default function App() {
  const activeSessionId = useCrowStore((s) => s.activeSessionId);
  const activeSessionPath = useCrowStore((s) => s.activeSessionPath);
  const agentState = useCrowStore((s) => s.agentState);
  const applyLiveEvent = useCrowStore((s) => s.applyLiveEvent);
  const addApproval = useCrowStore((s) => s.addApproval);
  const setSessions = useCrowStore((s) => s.setSessions);

  const showRail = useMediaQuery("(min-width: 900px)");
  const showInlineInspector = useMediaQuery("(min-width: 1100px)");

  const eventChannelRef = useRef<Channel<AgentEvent> | null>(null);
  const askChannelRef = useRef<Channel<AskNotification> | null>(null);

  useEffect(() => {
    const ec = new Channel<AgentEvent>();
    ec.onmessage = (event) => {
      applyLiveEvent(event);
    };
    eventChannelRef.current = ec;

    const ac = new Channel<AskNotification>();
    ac.onmessage = (notification) => {
      addApproval({
        ask_id: notification.ask_id,
        name: notification.call.name,
        args: notification.call.args,
      });
    };
    askChannelRef.current = ac;
  }, [applyLiveEvent, addApproval]);

  useEffect(() => {
    client
      .sessionList(".")
      .then((result) => {
        setSessions(result.sessions);
      })
      .catch(() => {});
  }, [setSessions]);

  const handleSubmit = useCallback(
    async (text: string) => {
      if (!activeSessionId || !activeSessionPath) return;
      const ec = eventChannelRef.current;
      const ac = askChannelRef.current;
      if (!ec || !ac) return;
      try {
        await client.submit(activeSessionId, activeSessionPath, text, ec, ac);
      } catch (err) {
        console.error("submit failed:", err);
      }
    },
    [activeSessionId, activeSessionPath],
  );

  const handleStop = useCallback(async () => {
    if (!activeSessionId) return;
    try {
      await client.interrupt(activeSessionId);
    } catch (err) {
      console.error("interrupt failed:", err);
    }
  }, [activeSessionId]);

  const isRunning =
    agentState === "sampling" || agentState === "executing_tool";

  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden bg-obsidian">
      <motion.div
        initial={{ opacity: 0, y: -12 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{
          type: "spring",
          stiffness: 300,
          damping: 30,
          delay: 0,
        }}
      >
        <TopBar />
      </motion.div>

      <div className="flex min-h-0 flex-1 gap-3 p-3">
        {showRail && (
          <motion.div
            initial={{ opacity: 0, x: -16 }}
            animate={{ opacity: 1, x: 0 }}
            transition={{
              type: "spring",
              stiffness: 300,
              damping: 30,
              delay: 0.05,
            }}
            className="shrink-0"
          >
            <SessionRail />
          </motion.div>
        )}

        <div className="flex min-w-0 flex-1 flex-col">
          <ConversationStream />
          <Composer
            onSubmit={handleSubmit}
            onStop={handleStop}
            isRunning={isRunning}
          />
        </div>

        {showInlineInspector && (
          <motion.div
            initial={{ opacity: 0, x: 16 }}
            animate={{ opacity: 1, x: 0 }}
            transition={{
              type: "spring",
              stiffness: 300,
              damping: 30,
              delay: 0.1,
            }}
            className="shrink-0"
          >
            <Inspector />
          </motion.div>
        )}
      </div>

      {!showInlineInspector && <Inspector />}

      <ApprovalOverlay />
    </div>
  );
}
