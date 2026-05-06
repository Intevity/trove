import { z } from 'zod';

// Placeholder IPC message contracts for Sprint 0. The real IPC surface
// (list_detected_harnesses, preview_patch, apply_patch, revert_patch, etc.)
// arrives in Sprint 3 alongside the detect+adapter sweep. Until then we ship
// a minimal Ping/Pong pair so the workspace types resolve end-to-end.

export const PingRequest = z.object({
  kind: z.literal('ping'),
  nonce: z.string().min(1),
});
export type PingRequest = z.infer<typeof PingRequest>;

export const PongResponse = z.object({
  kind: z.literal('pong'),
  nonce: z.string().min(1),
  appVersion: z.string().min(1),
});
export type PongResponse = z.infer<typeof PongResponse>;
