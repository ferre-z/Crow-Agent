import { motion } from "framer-motion";
import type { StreamEntry } from "../store";

interface UserBubbleProps {
  entry: StreamEntry & { kind: "user" };
}

export default function UserBubble({ entry }: UserBubbleProps) {
  return (
    <motion.div
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.2 }}
      className="flex justify-end px-4"
    >
      <div className="clay-inset max-w-[75%] px-4 py-3 text-sm leading-relaxed text-mist">
        {entry.content}
      </div>
    </motion.div>
  );
}
