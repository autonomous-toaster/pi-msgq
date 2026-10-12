// Test fixture: opens a blocking confirm prompt on demand, so the presence
// "blocked" state can be observed without stalling RPC startup.
// Loaded by scripts/e2e.mjs alongside the real extension.
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

export default function (pi: ExtensionAPI) {
	pi.registerCommand("blockme", {
		description: "Open a blocking prompt",
		handler: async (_args, ctx) => {
			await ctx.ui.confirm("msgq probe", "hold a blocking prompt open");
		},
	});
}
