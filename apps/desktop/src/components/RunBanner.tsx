import { motion } from "framer-motion";
import type { StreamEntry } from "../store";

interface RunBannerProps {
  entry: StreamEntry & {
    kind: "run_finished" | "run_failed" | "run_cancelled";
  };
}

export default function RunBanner({ entry }: RunBannerProps) {
  let message = "";
  let colorClass = "";

  if (entry.kind === "run_finished") {
    message = entry.message || "Run completed";
    colorClass = "text-halo";
  } else if (entry.kind === "run_failed") {
    message = `Failed: ${entry.message}`;
    colorClass = "text-coral";
  } else {
    message = "Run cancelled";
    colorClass = "text-fog";
  }

  return (
    <motion.div
      initial={{ opacity: 0, y: 4 }}
      animate={{ opacity: 1, y: 0 }}
      className="flex justify-center px-4 py-3"
    >
      <div className={`text-xs font-medium ${colorClass}`}>{message}</div>
    </motion.div>
  );
}
