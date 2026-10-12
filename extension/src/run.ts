// The one place that runs a planned command and turns the result into words.
//
// The model's tools and the human's commands delegate to the same binaries, so
// they share this. It never throws: a caller is either a tool reporting to a
// model or a command reporting to a person, and neither wants an exception.

/** A command to run. */
export interface PlannedCommand {
	command: string;
	args: string[];
}

/** What a run amounts to, in words a caller can hand back unchanged. */
export function summarizeRun(options: {
	command: string;
	code: number;
	stdout?: string;
	stderr?: string;
	binEnv: string;
}): string {
	const out = (options.stdout ?? "").trim();
	const err = (options.stderr ?? "").trim();
	if (options.code === 0) return out || "(no output)";

	// A failure with no output almost always means the binary was not found, and
	// naming the variable to set is the caller's actual fix.
	const detail = err || out;
	if (detail) return `${options.command} exited with ${options.code}: ${detail}`;
	return (
		`${options.command} exited with ${options.code} and said nothing, which usually means ` +
		`it was not found. Check that "${options.command}" is on PATH, or set ${options.binEnv} to ` +
		"its full path (in a development checkout they are not installed)."
	);
}

/** The slice of the host this module needs. */
export interface ExecHost {
	exec(
		command: string,
		args: string[],
		options?: { signal?: AbortSignal; timeout?: number },
	): Promise<{ stdout: string; stderr: string; code: number }>;
}

/** Run a planned command and describe what happened. Never throws. */
export async function runPlanned(
	pi: ExecHost,
	plan: PlannedCommand,
	options: { signal?: AbortSignal; timeoutMs?: number; binEnv: string },
): Promise<{ ok: boolean; text: string }> {
	try {
		const result = await pi.exec(plan.command, plan.args, {
			timeout: options.timeoutMs,
			signal: options.signal,
		});
		return {
			ok: result.code === 0,
			text: summarizeRun({
				command: plan.command,
				code: result.code,
				stdout: result.stdout,
				stderr: result.stderr,
				binEnv: options.binEnv,
			}),
		};
	} catch (error) {
		return { ok: false, text: `could not run ${plan.command}: ${String(error)}` };
	}
}
