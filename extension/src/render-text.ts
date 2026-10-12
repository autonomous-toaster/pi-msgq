// Pure text composition for msgq messages.
//
// No runtime imports, so this stays unit-testable without a TUI. The component
// wrapper lives in render.ts.

import type { Theme, ThemeColor } from "@earendil-works/pi-coding-agent";

/** The theme methods this module needs. */
export type MsgqTheme = Pick<Theme, "fg" | "bold">;

/** One delivered record, as the renderer sees it. */
export interface MsgqItem {
	from: string;
	/** The address the record was received on. */
	address: string;
	type: string;
	body: string;
}

/** Structured details attached to every msgq message. */
export interface MsgqDetails {
	items: MsgqItem[];
}

/** A color per record type, so the badge reads at a glance. */
export function typeColor(type: string): ThemeColor {
	switch (type) {
		case "prompt":
			return "accent";
		case "steer":
			return "warning";
		case "follow_up":
			return "accent";
		case "note":
			return "muted";
		case "event":
			return "dim";
		default:
			return "dim";
	}
}

function route(item: MsgqItem, theme: MsgqTheme): string {
	return `${theme.fg("accent", item.from)} ${theme.fg("dim", "\u2192")} ${theme.fg("accent", item.address)}`;
}

/**
 * How a human answers this message.
 *
 * Presentation only, and deliberately not part of the content: that is what the model
 * reads, and a line telling somebody to type a command would be addressed to the wrong
 * reader. It names the sender rather than pointing at `/reply`, because `/reply` means
 * "the message I most recently received", which is not the row in front of you once a
 * batch has several.
 */
function replyHint(item: MsgqItem, theme: MsgqTheme): string {
	// `?` is what the renderer substitutes when a message carries no details.
	if (!item.from || item.from === "?") return "";
	return theme.fg("dim", `reply: /peer ${item.from} ...`);
}

/**
 * Compose the body shown in the transcript.
 *
 * Single message:  `msgq note` / route / body
 * Batch:           `msgq · 3 messages` / numbered route + body blocks
 */
export function composeMsgqText(details: MsgqDetails, theme: MsgqTheme): string {
	const items = details.items.length > 0 ? details.items : [];
	const label = theme.fg("customMessageLabel", theme.bold("msgq"));
	const lines: string[] = [];

	if (items.length === 0) {
		return label;
	}

	if (items.length === 1) {
		const item = items[0];
		lines.push(`${label} ${theme.fg(typeColor(item.type), item.type)}`);
		lines.push(route(item, theme));
		if (item.body) {
			lines.push("");
			for (const line of item.body.split("\n")) {
				lines.push(theme.fg("customMessageText", line));
			}
		}
		const hint = replyHint(item, theme);
		if (hint) lines.push("", hint);
		return lines.join("\n");
	}

	lines.push(`${label} ${theme.fg("dim", `\u00b7 ${items.length} messages`)}`);
	items.forEach((item, index) => {
		lines.push("");
		lines.push(
			`${theme.fg("dim", `${index + 1}.`)} ${route(item, theme)} ${theme.fg(typeColor(item.type), item.type)}`,
		);
		for (const line of item.body.split("\n")) {
			lines.push(theme.fg("customMessageText", line));
		}
		// Each row names its own sender, so any one of them can be answered.
		const hint = replyHint(item, theme);
		if (hint) lines.push(hint);
	});
	return lines.join("\n");
}
