// TUI component for msgq messages. Registered with pi.registerMessageRenderer.

import type { CustomMessage, Theme } from "@earendil-works/pi-coding-agent";
import { Box, Text } from "@earendil-works/pi-tui";

import { composeMsgqText, type MsgqDetails } from "./render-text.ts";

/**
 * Render a msgq custom message in the transcript: a labelled header, the route
 * between sender and address, then the body, on the custom-message background.
 */
export function renderMsgq(
	message: CustomMessage<MsgqDetails>,
	options: { outputPad: number },
	theme: Theme,
) {
	const details: MsgqDetails = message.details?.items?.length
		? message.details
		: { items: [{ from: "?", address: "?", type: "note", body: String(message.content ?? "") }] };

	const box = new Box(options.outputPad, 1, (text) => theme.bg("customMessageBg", text));
	box.addChild(new Text(composeMsgqText(details, theme), 0, 0));
	return box;
}
