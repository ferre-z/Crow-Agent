import { z } from "zod";

/** A saved daemon host entry (persisted in userData/hosts.json). */
export const knownHostSchema = z.object({
  name: z.string().min(1),
  url: z.string().min(1),
  token: z.string().min(1),
});
export type KnownHost = z.infer<typeof knownHostSchema>;

export const hostsFileSchema = z.array(knownHostSchema);
