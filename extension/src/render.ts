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

	// Purple is what a record the session acted on looks like. A display-only record keeps the plain
	// background, so the difference between "woke the agent" and "was shown" is visible at a glance
	// rather than being something you have to remember the policy to know.
	const honored = details.items.some((item) => item.honored !== false);
	const box = honored
		? new Box(options.outputPad, 1, (text) => theme.bg("customMessageBg", text))
		: new Box(options.outputPad, 1);
	box.addChild(new Text(composeMsgqText(details, theme), 0, 0));
	return box;
}
