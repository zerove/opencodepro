import { z } from "zod";

/**
 * Example custom tool: Greet a user.
 *
 * Drop this file into .opencode/tools/greet.ts to make it available to the agent.
 *
 * Each tool must export:
 *   - Schema: a Zod type for argument validation
 *   - handler: an async function (args) => result
 */

export const Schema = z
  .object({
    name: z.string().min(1).describe("Name of the person to greet"),
    title: z.string().optional().describe("Optional title (e.g., Dr., Captain)"),
    excitement: z
      .number()
      .min(1)
      .max(10)
      .default(3)
      .describe("Excitement level (1-10)"),
  })
  .describe("Greet someone warmly");

export async function handler(args: z.infer<typeof Schema>): Promise<string> {
  const prefix = args.title ? `${args.title} ` : "";
  const exclamation = "!".repeat(args.excitement);
  return `Hello, ${prefix}${args.name}${exclamation}`;
}
