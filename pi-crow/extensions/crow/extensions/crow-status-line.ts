/**
 * crow-status-line — replaces the default pi footer with one
 * that's branded Crow + shows session metadata that's useful when
 * running long sessions on Nemotron 3 Ultra.
 *
 * Default-off: enabled by `/crow-status` slash command so users
 * who prefer the upstream footer aren't forced into the change.
 *
 * Format (single line, fits 120-col terminals):
 *   ↑1.2k ↓800 $0.0123  42% ctx  nemotron-3-ultra-550b-a55b
 *
 * Pieces:
 *   - cumulative token + cost (from session history)
 *   - context-window usage % (from pricing.toml context_size)
 *   - active model family (strips the vendor prefix)
 *   - git branch (when in a git repo) — courtesy of pi's FooterDataProvider
 */

import type { AssistantMessage } from "@crow/pi-ai";
import type { ExtensionAPI } from "@crow/coding-agent";
import { truncateToWidth, visibleWidth } from "@crow/pi-tui";

const PRICING: Record<string, { input: number; output: number; ctx: number }> = {
	// Per-1K-token USD rates + known context windows for the
	// models Crow ships with as defaults. Mirrors the pricing.toml
	// from the old Rust kernel. Edit freely; values here take
	// precedence so we don't depend on the user's settings.
	"nvidia/nemotron-3-ultra-550b-a55b": {
		input: 0.0005,
		output: 0.0015,
		ctx: 262_144,
	},
	"nvidia/llama-3.1-nemotron-ultra-253b-v1": {
		input: 0.0006,
		output: 0.0006,
		ctx: 131_072,
	},
	"meta/llama-3.1-70b-instruct": {
		input: 0.00059,
		output: 0.00079,
		ctx: 131_072,
	},
	__default: { input: 0.001, output: 0.003, ctx: 32_768 },
};

function pricing(modelId: string) {
	return PRICING[modelId] ?? PRICING.__default;
}

function fmt(n: number): string {
	if (n < 1000) return `${n}`;
	if (n < 10_000) return `${(n / 1000).toFixed(2)}k`;
	return `${(n / 1000).toFixed(1)}k`;
}

function modelFamily(modelId: string): string {
	const slash = modelId.lastIndexOf("/");
	return slash >= 0 ? modelId.slice(slash + 1) : modelId;
}

export default function (pi: ExtensionAPI) {
	let enabled = false;
	let lastCtxPct = 0;

	function recompute() {
		// Compute cumulative tokens + cost across every assistant
		// message in the active session branch. Mirrors what the
		// Rust kernel's per_tool_tokens bucket did, but as a sum.
		let input = 0,
			output = 0;
		for (const entry of pi.sessionManager.getBranch()) {
			if (entry.type !== "message") continue;
			const m = entry.message;
			if (m.role !== "assistant") continue;
			const a = m as AssistantMessage;
			input += a.usage?.input ?? 0;
			output += a.usage?.output ?? 0;
		}
		const modelId = pi.model?.id ?? "unknown";
		const p = pricing(modelId);
		const cost = (input / 1000) * p.input + (output / 1000) * p.output;
		const lastIn = pi.sessionManager
			.getBranch()
			.filter((e) => e.type === "message" && e.message.role === "assistant")
			.map((e) => (e.message as AssistantMessage).usage?.input ?? 0)
			.pop() ?? 0;
		lastCtxPct = p.ctx > 0 ? Math.round((lastIn / p.ctx) * 100) : 0;
		return { input, output, cost, modelId };
	}

	pi.registerCommand("crow-status", {
		description: "Toggle Crow session metadata in the footer (tokens, cost, context %, model)",
		handler: async (_args, ctx) => {
			enabled = !enabled;
			if (enabled) {
				ctx.ui.setFooter((tui, theme, footerData) => {
					const unsubBranch = footerData.onBranchChange(() =>
						tui.requestRender(),
					);
					return {
						dispose: unsubBranch,
						invalidate() {},
						render(width: number): string[] {
							const { input, output, cost, modelId } = recompute();
							const branch = footerData.getGitBranch();
							const left = theme.fg(
								"dim",
								`↑${fmt(input)} ↓${fmt(output)} $${cost.toFixed(4)} ${lastCtxPct}% ctx`,
							);
							const family = modelFamily(modelId);
							const right = theme.fg(
								"dim",
								branch ? `${family} (${branch})` : family,
							);
							const pad = " ".repeat(
								Math.max(1, width - visibleWidth(left) - visibleWidth(right)),
							);
							return [truncateToWidth(left + pad + right, width)];
						},
					};
				});
				ctx.ui.notify("Crow status line enabled", "info");
			} else {
				ctx.ui.setFooter(undefined);
				ctx.ui.notify("Default footer restored", "info");
			}
		},
	});
}
