import { motion, AnimatePresence, useReducedMotion } from "framer-motion";
import { askResolve } from "../ipc/client";
import { useCrowStore } from "../store";

export default function ApprovalOverlay() {
  const queue = useCrowStore((s) => s.approvalQueue);
  const removeApproval = useCrowStore((s) => s.removeApproval);
  const reducedMotion = useReducedMotion();

  const current = queue[0];

  const handleDecision = async (decision: "allow" | "deny") => {
    if (!current) return;
    try {
      await askResolve(current.ask_id, decision);
    } catch (err) {
      console.error("ask_resolve failed:", err);
    }
    removeApproval(current.ask_id);
  };

  return (
    <AnimatePresence>
      {current && (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: reducedMotion ? 0 : 0.2 }}
          className="fixed inset-0 z-50 flex items-center justify-center"
          style={{
            backgroundColor: "rgba(0,0,0,0.5)",
            backdropFilter: "blur(4px)",
          }}
        >
          <motion.div
            initial={{ scale: 0.85, opacity: 0 }}
            animate={{ scale: 1, opacity: 1 }}
            exit={{ scale: 0.85, opacity: 0 }}
            transition={
              reducedMotion
                ? { duration: 0 }
                : { type: "spring", stiffness: 400, damping: 25 }
            }
            className="clay-card mx-4 w-full max-w-md p-6"
          >
            <div className="mb-1 font-display text-sm font-semibold text-mist">
              Approval Required
            </div>
            <div className="mb-4 text-xs text-fog">Crow wants to run:</div>

            <div className="clay-inset mb-5 rounded-xl p-4">
              <div className="font-mono text-sm font-medium text-mist">
                {current.name}
              </div>
              {current.args != null && (
                <pre className="mt-2 max-h-32 overflow-auto font-mono text-xs text-fog whitespace-pre-wrap">
                  {JSON.stringify(current.args, null, 2)}
                </pre>
              )}
            </div>

            <div className="flex justify-end gap-3">
              <button
                type="button"
                onClick={() => handleDecision("deny")}
                className="rounded-xl px-5 py-2 text-sm font-medium text-fog transition-colors hover:bg-white/5 hover:text-mist"
              >
                Deny
              </button>
              <button
                type="button"
                onClick={() => handleDecision("allow")}
                className="rounded-xl bg-sheen/20 px-5 py-2 text-sm font-medium text-sheen transition-colors hover:bg-sheen/30"
              >
                Allow
              </button>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
