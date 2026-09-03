//! Phone-board Worker: pairing, heartbeats, and authenticated on/off.
//!
//! The Durable Object wrapper lives in `index.js`. This module is the policy
//! the tests lock — no Cloudflare runtime required.

export const HEARTBEAT_TTL_SECS = 15;
export const PAIRING_TTL_SECS = 10 * 60;
export const PAIRING_CODE_LEN = 8;
const CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

export function tokensMatch(left, right) {
  if (typeof left !== "string" || typeof right !== "string") return false;
  if (left.length !== right.length) return false;
  let acc = 0;
  for (let i = 0; i < left.length; i += 1) {
    acc |= left.charCodeAt(i) ^ right.charCodeAt(i);
  }
  return acc === 0;
}

export function normalizePairingCode(raw) {
  if (typeof raw !== "string") return null;
  let out = "";
  for (const ch of raw) {
    if (ch === "-" || ch.trim() === "") continue;
    const up = ch.toUpperCase();
    const mapped = up === "I" || up === "L" ? "1" : up === "O" ? "0" : up;
    if (!CROCKFORD.includes(mapped)) return null;
    out += mapped;
    if (out.length > PAIRING_CODE_LEN) return null;
  }
  return out.length === PAIRING_CODE_LEN ? out : null;
}

export function formatPairingCode(code) {
  if (code.length === PAIRING_CODE_LEN) {
    return `${code.slice(0, 4)}-${code.slice(4)}`;
  }
  return code;
}

export function pairingUrl(code, chinese) {
  const path = chinese ? "/zh/board/" : "/board/";
  return `https://xyz-ai.app/never-sleep${path}?code=${formatPairingCode(code)}`;
}

export function deviceIsOnline(lastSeenUnix, nowUnix) {
  if (nowUnix < lastSeenUnix) return true;
  return nowUnix - lastSeenUnix <= HEARTBEAT_TTL_SECS;
}

function randomHex(bytes) {
  const buf = new Uint8Array(bytes);
  crypto.getRandomValues(buf);
  return [...buf].map((b) => b.toString(16).padStart(2, "0")).join("");
}

function pairingCodeFromRandom() {
  const buf = new Uint8Array(5);
  crypto.getRandomValues(buf);
  let bits = 0n;
  for (const b of buf) bits = (bits << 8n) | BigInt(b);
  let out = "";
  for (let i = 0; i < PAIRING_CODE_LEN; i += 1) {
    out = CROCKFORD[Number(bits & 31n)] + out;
    bits >>= 5n;
  }
  return out;
}

function jsonResponse(body, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json; charset=utf-8" },
  });
}

function emptyStatus() {
  return {
    active: false,
    display: "awake",
    lid: "open",
    on_ac: true,
    battery: null,
    remaining_secs: null,
    user_present: false,
    elapsed_secs: null,
    stop_reason: null,
    stop_reason_code: null,
    screen_off_enabled: true,
    lid_awake_enabled: true,
  };
}

export class Board {
  constructor(nowSecs = () => Math.floor(Date.now() / 1000)) {
    this.nowSecs = nowSecs;
    /** @type {Map<string, object>} */
    this.devices = new Map();
    /** @type {Map<string, { deviceId: string, expires: number }>} */
    this.codes = new Map();
  }

  static fromJSON(data) {
    const board = new Board();
    if (!data) return board;
    for (const [id, device] of Object.entries(data.devices || {})) {
      board.devices.set(id, device);
    }
    for (const [code, offer] of Object.entries(data.codes || {})) {
      board.codes.set(code, offer);
    }
    return board;
  }

  toJSON() {
    return {
      devices: Object.fromEntries(this.devices),
      codes: Object.fromEntries(this.codes),
    };
  }

  #purgeCodes(now) {
    for (const [code, offer] of this.codes) {
      if (offer.expires <= now) this.codes.delete(code);
    }
  }

  startPairing({ deviceId, deviceToken, displayName }) {
    const now = this.nowSecs();
    if (
      typeof deviceId !== "string" ||
      typeof deviceToken !== "string" ||
      deviceId.length < 16 ||
      deviceToken.length < 16
    ) {
      return { ok: false, error: "bad_identity", status: 400 };
    }
    const existing = this.devices.get(deviceId);
    if (existing && !tokensMatch(existing.token, deviceToken)) {
      return { ok: false, error: "unauthorized", status: 401 };
    }
    if (!existing) {
      this.devices.set(deviceId, {
        token: deviceToken,
        displayName: displayName || "Mac",
        lastSeen: null,
        status: emptyStatus(),
        commands: [],
      });
    } else if (displayName) {
      existing.displayName = displayName;
    }
    this.#purgeCodes(now);
    for (const [code, offer] of this.codes) {
      if (offer.deviceId === deviceId) this.codes.delete(code);
    }
    const code = pairingCodeFromRandom();
    this.codes.set(code, { deviceId, expires: now + PAIRING_TTL_SECS });
    return {
      ok: true,
      pairing_code: formatPairingCode(code),
      pairing_url: pairingUrl(code, false),
      expires_unix: now + PAIRING_TTL_SECS,
      status: 200,
    };
  }

  claim(rawCode) {
    const now = this.nowSecs();
    this.#purgeCodes(now);
    const code = normalizePairingCode(rawCode);
    if (!code) {
      return { ok: false, error: "unknown_code", status: 404 };
    }
    const offer = this.codes.get(code);
    if (!offer) {
      return { ok: false, error: "unknown_code", status: 404 };
    }
    const device = this.devices.get(offer.deviceId);
    if (!device) {
      return { ok: false, error: "unknown_code", status: 404 };
    }
    return {
      ok: true,
      device_id: offer.deviceId,
      device_token: device.token,
      display_name: device.displayName,
      status: 200,
    };
  }

  heartbeat({ deviceId, deviceToken, displayName, status }) {
    const now = this.nowSecs();
    const device = this.devices.get(deviceId);
    if (!device || !tokensMatch(device.token, deviceToken)) {
      return { ok: false, error: "unauthorized", status: 401 };
    }
    if (displayName) device.displayName = displayName;
    if (status && typeof status === "object") device.status = status;
    device.lastSeen = now;
    const pending = device.commands || [];
    device.commands = [];
    this.#purgeCodes(now);
    let pairing = null;
    for (const [code, offer] of this.codes) {
      if (offer.deviceId === deviceId) {
        pairing = {
          pairing_code: formatPairingCode(code),
          pairing_url: pairingUrl(code, false),
        };
        break;
      }
    }
    return {
      ok: true,
      commands: pending,
      pairing_code: pairing?.pairing_code || null,
      pairing_url: pairing?.pairing_url || null,
      status: 200,
    };
  }

  list(entries) {
    const now = this.nowSecs();
    const devices = [];
    if (!Array.isArray(entries)) {
      return { ok: false, error: "bad_request", status: 400 };
    }
    for (const entry of entries) {
      const deviceId = entry?.device_id;
      const token = entry?.device_token;
      const device = this.devices.get(deviceId);
      if (!device || !tokensMatch(device.token, token)) {
        continue;
      }
      const online =
        device.lastSeen != null && deviceIsOnline(device.lastSeen, now);
      devices.push({
        device_id: deviceId,
        display_name: device.displayName,
        online,
        last_seen_unix: device.lastSeen,
        ...device.status,
      });
    }
    return { ok: true, devices, status: 200 };
  }

  command({ deviceId, deviceToken, cmd, duration }) {
    const now = this.nowSecs();
    const device = this.devices.get(deviceId);
    if (!device || !tokensMatch(device.token, deviceToken)) {
      return { ok: false, error: "unauthorized", status: 401 };
    }
    if (cmd !== "on" && cmd !== "off") {
      return { ok: false, error: "bad_cmd", status: 400 };
    }
    const online =
      device.lastSeen != null && deviceIsOnline(device.lastSeen, now);
    if (!online) {
      return { ok: false, error: "offline", accepted: false, status: 409 };
    }
    const item = {
      id: randomHex(8),
      cmd,
    };
    if (cmd === "on" && duration) item.duration = duration;
    device.commands = device.commands || [];
    device.commands.push(item);
    return { ok: true, accepted: true, command_id: item.id, status: 200 };
  }
}

export async function handleApi(board, request) {
  const url = new URL(request.url);
  const path = url.pathname.replace(/\/+$/, "") || "/";
  if (request.method !== "POST") {
    return jsonResponse({ ok: false, error: "method" }, 405);
  }
  let body = {};
  try {
    body = await request.json();
  } catch {
    body = {};
  }
  let result;
  switch (path) {
    case "/api/pair/start":
      result = board.startPairing({
        deviceId: body.device_id,
        deviceToken: body.device_token,
        displayName: body.display_name,
      });
      break;
    case "/api/pair/claim":
      result = board.claim(body.pairing_code);
      break;
    case "/api/heartbeat":
      result = board.heartbeat({
        deviceId: body.device_id,
        deviceToken: body.device_token,
        displayName: body.display_name,
        status: body.status,
      });
      break;
    case "/api/list":
      result = board.list(body.devices);
      break;
    case "/api/command":
      result = board.command({
        deviceId: body.device_id,
        deviceToken: body.device_token,
        cmd: body.cmd,
        duration: body.duration,
      });
      break;
    default:
      return jsonResponse({ ok: false, error: "not_found" }, 404);
  }
  const { status, ...payload } = result;
  return jsonResponse(payload, status);
}
