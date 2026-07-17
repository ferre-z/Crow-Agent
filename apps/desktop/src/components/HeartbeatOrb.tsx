import { motion, useReducedMotion } from "framer-motion";
import type { AgentState } from "../store";

interface HeartbeatOrbProps {
  state: AgentState;
}

const stateColors: Record<AgentState, string> = {
  idle: "var(--sheen)",
  sampling: "var(--iris)",
  executing_tool: "var(--halo)",
  failed: "var(--coral)",
};

const stateScale: Record<AgentState, [number, number]> = {
  idle: [1, 1.05],
  sampling: [1, 1.08],
  executing_tool: [1, 1.12],
  failed: [1, 1],
};

export default function HeartbeatOrb({ state }: HeartbeatOrbProps) {
  const reducedMotion = useReducedMotion();

  const color = stateColors[state];
  const [min, mid] = stateScale[state];

  const duration =
    state === "idle" ? 3 : state === "sampling" ? 1.8 : 1.2;

  const isActive = state !== "failed";

  return (
    <motion.div
      className="relative flex h-5 w-5 items-center justify-center"
      aria-label={`Agent status: ${state}`}
    >
      {isActive && !reducedMotion && (
        <motion.div
          className="absolute inset-0 rounded-full"
          style={{ backgroundColor: color, opacity: 0.25 }}
          animate={{
            scale: [min, mid + 0.15, min],
            opacity: [0.2, 0.35, 0.2],
          }}
          transition={{
            duration: duration * 1.3,
            repeat: Infinity,
            ease: "easeInOut",
          }}
        />
      )}
      <motion.div
        className="relative h-3 w-3 rounded-full"
        style={{
          backgroundColor: color,
          boxShadow: `0 0 12px ${color}`,
        }}
        animate={
          reducedMotion
            ? { opacity: state === "failed" ? [0.4, 1, 0.4] : 1 }
            : state === "failed"
              ? {
                  opacity: [1, 0.3, 1],
                  boxShadow: [
                    `0 0 8px ${color}`,
                    `0 0 2px ${color}`,
                    `0 0 8px ${color}`,
                  ],
                }
              : {
                  scale: [min, mid, min],
                }
        }
        transition={
          reducedMotion
            ? { duration: 2, repeat: Infinity }
            : state === "failed"
              ? { duration: 1.5, repeat: Infinity }
              : {
                  duration,
                  repeat: Infinity,
                  ease: "easeInOut",
                }
        }
      />
      {state === "executing_tool" && !reducedMotion && (
        <div
          className="absolute inset-[-4px] rounded-full"
          style={{
            background:
              "conic-gradient(from var(--angle), var(--sheen), var(--iris), var(--halo), var(--sheen))",
            opacity: 0.6,
            animation: "rotate-angle 3s linear infinite",
          }}
        />
      )}
    </motion.div>
  );
}
