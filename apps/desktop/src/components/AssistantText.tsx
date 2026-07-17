import { motion } from "framer-motion";
import type { StreamEntry } from "../store";

interface AssistantTextProps {
  entry: StreamEntry & { kind: "assistant" };
}

export default function AssistantText({ entry }: AssistantTextProps) {
  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      transition={{ duration: 0.15 }}
      className="px-4 py-2"
    >
      <div className="max-w-none font-sans text-base leading-relaxed text-mist whitespace-pre-wrap">
        {entry.text}
      </div>
    </motion.div>
  );
}
