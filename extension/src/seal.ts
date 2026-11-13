// Sealing a record body.
//
// The same cipher, key derivation and record fields as the CLI: one authenticated cipher per
// principal, keyed from a passphrase, with the algorithm, a key identifier, a nonce and the field
// name travelling in `meta.enc`. A body sealed by either client opens in the other.

import { createCipheriv, createDecipheriv, createHash, randomBytes, scryptSync } from "node:crypto";
import { readFileSync } from "node:fs";

/** The authenticated cipher this build uses. */
export const ALG = "aes-256-gcm";

export const PASSPHRASE_ENV = "MSGQ_PASSPHRASE";
export const PASSPHRASE_FILE_ENV = "MSGQ_PASSPHRASE_FILE";
export const SALT_ENV = "MSGQ_KEY_SALT";
export const DEFAULT_SALT = "msgq-encryption/v1";

/** Cost of the key derivation, matching the CLI. */
const LOG_N = 15;
const NONCE_BYTES = 12;

/** A derived key, its identifier, and where the passphrase came from. */
export interface SealingKey {
	kid: string;
	bytes: Buffer;
	source: string;
}

/** A stable name for a key generation: the hash of the salt, never of the key. */
export function kidOf(salt: string): string {
	return createHash("sha256").update(salt, "utf8").digest("hex").slice(0, 16);
}

/** Derive a key from a passphrase. */
export function deriveKey(passphrase: string, salt: string = DEFAULT_SALT): SealingKey {
	const bytes = scryptSync(passphrase, salt, 32, {
		N: 1 << LOG_N,
		r: 8,
		p: 1,
		// scrypt needs 128 * N * r bytes; the default cap is smaller than one derivation.
		maxmem: 128 * (1 << LOG_N) * 8 * 2,
	});
	return { kid: kidOf(salt), bytes, source: "environment" };
}

/**
 * The configured key, or nothing when no passphrase is configured.
 *
 * A configured source that cannot be read is thrown, not ignored: a deployment that meant to encrypt
 * and did not is the failure this refuses.
 */
export function loadKey(
	env: Record<string, string | undefined> = process.env,
	read: (path: string) => string = (path) => readFileSync(path, "utf8"),
): SealingKey | undefined {
	const salt = env[SALT_ENV]?.trim() || DEFAULT_SALT;
	const direct = env[PASSPHRASE_ENV]?.trim();
	if (direct) return deriveKey(direct, salt);
	const path = env[PASSPHRASE_FILE_ENV]?.trim();
	if (!path) return undefined;
	const passphrase = read(path).trim();
	if (!passphrase) throw new Error(`${PASSPHRASE_FILE_ENV} names ${path} but the file is empty`);
	return { ...deriveKey(passphrase, salt), source: `file ${path}` };
}

/** Seal one body. Returns the nonce and the ciphertext, both base64. */
export function sealBytes(
	plaintext: Buffer,
	aad: Buffer,
	key: SealingKey,
	nonce: Buffer = randomBytes(NONCE_BYTES),
): { nonce: string; body: string } {
	const cipher = createCipheriv(ALG, key.bytes, nonce);
	cipher.setAAD(aad);
	const body = Buffer.concat([cipher.update(plaintext), cipher.final(), cipher.getAuthTag()]);
	return { nonce: nonce.toString("base64"), body: body.toString("base64") };
}

/** Open one body. A body that was altered fails here instead of being read as content. */
export function openBytes(nonce: string, body: string, aad: Buffer, key: SealingKey): Buffer {
	const nonceBytes = Buffer.from(nonce, "base64");
	if (nonceBytes.length !== NONCE_BYTES) throw new Error("the record carries no usable nonce");
	const sealed = Buffer.from(body, "base64");
	if (sealed.length < 16) throw new Error("the body is too short to be sealed");
	const decipher = createDecipheriv(ALG, key.bytes, nonceBytes);
	decipher.setAAD(aad);
	decipher.setAuthTag(sealed.subarray(sealed.length - 16));
	return Buffer.concat([decipher.update(sealed.subarray(0, sealed.length - 16)), decipher.final()]);
}

/** The additional data a sealed body is bound to: version and type, never the stamped sender. */
export function additionalData(record: Record<string, unknown>): Buffer {
	const version = typeof record.v === "number" ? record.v : 0;
	const kind = typeof record.type === "string" ? record.type : "";
	return Buffer.from(`msgq/${version}/${kind}`, "utf8");
}

/** How this deployment encrypts, for reporting. Never prints the passphrase or the key. */
export function describeKey(key?: SealingKey): string {
	if (!key) return "none (bodies are plaintext)";
	return `${ALG} (kid=${key.kid}, source=${key.source})`;
}
