//! Phone-board Worker: pairing, heartbeats, and authenticated on/off.
//!
//! The Durable Object wrapper lives in `index.js`. This module is the policy
//! the tests lock — no Cloudflare runtime required.

export const HEARTBEAT_TTL_SECS = 15;
export const PAIRING_TTL_SECS = 10 * 60;
export const PAIRING_CODE_LEN = 8;
const CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const PUBLIC_SITE_ORIGIN = "https://xyz-ai.app/never-sleep";

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

export function isChineseLang(lang) {
  if (typeof lang !== "string") return false;
  const primary = lang
    .trim()
    .toLowerCase()
    .replaceAll("_", "-")
    .split(/[-.@]/)[0];
  return (
    primary === "zh" ||
    primary === "chi" ||
    primary === "chinese" ||
    primary === "cn"
  );
}

export function publicSiteOrigin(requestUrl, env = {}) {
  const fromEnv = env.PUBLIC_SITE_ORIGIN || env.NEVER_SLEEP_CLOUD_URL;
  if (typeof fromEnv === "string" && fromEnv.trim()) {
    return fromEnv.trim().replace(/\/+$/, "");
  }
  let url;
  try {
    url = new URL(requestUrl);
  } catch {
    return PUBLIC_SITE_ORIGIN;
  }
  const host = url.hostname;
  if (host === "xyz-ai.app" || host === "www.xyz-ai.app") {
    return `${url.protocol}//${host}/never-sleep`;
  }
  return `${url.protocol}//${url.host}`;
}

export function pairingUrl(code, chinese, origin) {
  const base = (origin || PUBLIC_SITE_ORIGIN).replace(/\/+$/, "");
  const path = chinese ? "/zh/board/" : "/board/";
  return `${base}${path}?code=${formatPairingCode(code)}`;
}

export function pairingCodeIsLive(liveCode, candidateCode) {
  const live = normalizePairingCode(liveCode);
  const cand = normalizePairingCode(candidateCode);
  return Boolean(live && cand && live === cand);
}

export function deviceIsOnline(lastSeenUnix, nowUnix) {
  if (nowUnix < lastSeenUnix) return true;
  return nowUnix - lastSeenUnix <= HEARTBEAT_TTL_SECS;
}

export function shardName(path, body) {
  const p = (path || "").replace(/\/+$/, "") || "/";
  if (p === "/api/pair/claim") {
    const code = normalizePairingCode(body?.pairing_code);
    return code ? `pair:${code}` : null;
  }
  if (p === "/api/list") return null;
  const id = body?.device_id;
  if (typeof id === "string" && id.length >= 16) {
    return `device:${id}`;
  }
  return null;
}

export const LIST_MAX_DEVICES = 32;
export const PAIR_RESERVE_ATTEMPTS = 8;
const U32_MAX = 0xffffffff;
/** `JsonStatus` counters are `Option<u64>`; JSON numbers stay exact through 2^53-1. */
const MAX_STATUS_SECS = Number.MAX_SAFE_INTEGER;

export function capListEntries(entries) {
  if (!Array.isArray(entries)) return [];
  const seen = new Set();
  const out = [];
  for (const entry of entries) {
    const id = entry?.device_id;
    if (typeof id !== "string" || id.length < 16) continue;
    if (seen.has(id)) continue;
    seen.add(id);
    out.push(entry);
    if (out.length >= LIST_MAX_DEVICES) break;
  }
  return out;
}

export function boardHasState(data) {
  if (!data || typeof data !== "object") return false;
  const devices = data.devices || {};
  const codes = data.codes || {};
  return Object.keys(devices).length > 0 || Object.keys(codes).length > 0;
}

export function persistBoardAction(previous, next) {
  if (boardHasState(next)) return "put";
  if (previous == null) return "skip";
  return "delete";
}

/** Run async work one-at-a-time so overlapping callers cannot share a snapshot. */
export function createSerialQueue() {
  let tail = Promise.resolve();
  return (work) => {
    const run = tail.then(work);
    tail = run.then(
      () => undefined,
      () => undefined,
    );
    return run;
  };
}

function isUntilClock(raw) {
  const m = /^(\d{1,2}):(\d{2})$/.exec(raw);
  if (!m) return false;
  const hour = Number(m[1]);
  const minute = Number(m[2]);
  return hour <= 23 && minute <= 59;
}

function parseU32Hours(num) {
  if (typeof num !== "string" || !/^\d+$/.test(num) || num.length > 10) {
    return null;
  }
  const hours = Number(num);
  if (!Number.isInteger(hours) || hours < 1 || hours > U32_MAX) return null;
  return hours;
}

export function isAllowedDuration(raw) {
  if (typeof raw !== "string") return false;
  const s = raw.trim().toLowerCase();
  if (!s) return false;
  if (s === "indefinite" || s === "inf" || s === "forever" || s === "无限") {
    return true;
  }
  let clock = null;
  if (s.startsWith("until=")) clock = s.slice(6);
  else if (s.startsWith("until:")) clock = s.slice(6);
  else if (isUntilClock(s)) clock = s;
  if (clock != null) return isUntilClock(clock);
  if (s.endsWith("h")) return parseU32Hours(s.slice(0, -1)) != null;
  if (s.endsWith("小时")) return parseU32Hours(s.slice(0, -2).trim()) != null;
  return false;
}

export async function publishReservedPairing({
  generateCode,
  reserve,
  startDevice,
  release,
  maxAttempts = PAIR_RESERVE_ATTEMPTS,
}) {
  for (let i = 0; i < maxAttempts; i++) {
    const code = generateCode();
    const reserved = await reserve(code);
    if (!reserved?.ok) continue;
    const started = await startDevice(code);
    if (started?.ok) return started;
    if (release) await release(code);
  }
  return { ok: false, error: "pair_busy", status: 503 };
}

function asBool(value) {
  return value === true;
}

function asInt(value, min, max) {
  if (typeof value !== "number" || !Number.isFinite(value)) return null;
  const n = Math.round(value);
  if (n < min || n > max) return null;
  return n;
}

function asStopReasonCode(value) {
  if (typeof value !== "string") return null;
  return /^[a-z][a-z0-9_]{0,31}$/.test(value) ? value : null;
}

export function sanitizeStatus(raw) {
  const src = raw && typeof raw === "object" ? raw : {};
  return {
    active: asBool(src.active),
    display: src.display === "asleep" ? "asleep" : "awake",
    lid: src.lid === "closed" ? "closed" : "open",
    on_ac: asBool(src.on_ac),
    battery: asInt(src.battery, 0, 100),
    remaining_secs: asInt(src.remaining_secs, 0, MAX_STATUS_SECS),
    user_present: asBool(src.user_present),
    elapsed_secs: asInt(src.elapsed_secs, 0, MAX_STATUS_SECS),
    stop_reason: null,
    stop_reason_code: asStopReasonCode(src.stop_reason_code),
    screen_off_enabled: asBool(src.screen_off_enabled),
    lid_awake_enabled: asBool(src.lid_awake_enabled),
  };
}

function randomHex(bytes) {
  const buf = new Uint8Array(bytes);
  crypto.getRandomValues(buf);
  return [...buf].map((b) => b.toString(16).padStart(2, "0")).join("");
}

export function proposePairingCode() {
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

export function jsonResponse(body, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json; charset=utf-8" },
  });
}

function emptyStatus() {
  return sanitizeStatus({
    active: false,
    display: "awake",
    lid: "open",
    on_ac: true,
    screen_off_enabled: true,
    lid_awake_enabled: true,
  });
}

export class Board {
  constructor(nowSecs = () => Math.floor(Date.now() / 1000)) {
    this.nowSecs = nowSecs;
    /** @type {Map<string, object>} */
    this.devices = new Map();
    /** @type {Map<string, { deviceId: string, expires: number, token?: string, displayName?: string }>} */
    this.codes = new Map();
  }

  static fromJSON(data) {
    const board = new Board();
    if (!data) return board;
    for (const [id, device] of Object.entries(data.devices || {})) {
      board.devices.set(id, structuredClone(device));
    }
    for (const [code, offer] of Object.entries(data.codes || {})) {
      board.codes.set(code, structuredClone(offer));
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
    const expired = [];
    for (const [code, offer] of this.codes) {
      if (offer.expires <= now) {
        expired.push(code);
        this.codes.delete(code);
      }
    }
    return expired;
  }

  livePairing(deviceId) {
    const now = this.nowSecs();
    for (const [code, offer] of this.codes) {
      if (offer.deviceId === deviceId && offer.expires > now) {
        return {
          ok: true,
          pairing_code: formatPairingCode(code),
          status: 200,
        };
      }
    }
    return { ok: true, pairing_code: null, status: 200 };
  }

  peekOffer(rawCode) {
    const now = this.nowSecs();
    const code = normalizePairingCode(rawCode);
    if (!code) {
      return { ok: false, error: "unknown_code", status: 404 };
    }
    const offer = this.codes.get(code);
    if (!offer || offer.expires <= now) {
      if (code) this.codes.delete(code);
      return { ok: false, error: "unknown_code", status: 404 };
    }
    return { ok: true, device_id: offer.deviceId, status: 200 };
  }

  startPairing({
    deviceId,
    deviceToken,
    displayName,
    lang,
    origin,
    pairingCode,
    expiresUnix,
  }) {
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
    const replacedCodes = this.#purgeCodes(now);
    for (const [code, offer] of this.codes) {
      if (offer.deviceId === deviceId) {
        replacedCodes.push(code);
        this.codes.delete(code);
      }
    }
    let code;
    if (pairingCode != null && pairingCode !== "") {
      code = normalizePairingCode(pairingCode);
      if (!code) {
        return { ok: false, error: "bad_code", status: 400 };
      }
    } else {
      code = proposePairingCode();
    }
    const expires =
      typeof expiresUnix === "number" && Number.isFinite(expiresUnix)
        ? expiresUnix
        : now + PAIRING_TTL_SECS;
    this.codes.set(code, {
      deviceId,
      token: deviceToken,
      displayName: displayName || existing?.displayName || "Mac",
      expires,
    });
    const chinese = isChineseLang(lang);
    return {
      ok: true,
      pairing_code: formatPairingCode(code),
      pairing_url: pairingUrl(code, chinese, origin),
      expires_unix: expires,
      replaced_codes: replacedCodes,
      status: 200,
    };
  }

  rememberOffer({ pairingCode, deviceId, deviceToken, displayName, expiresUnix }) {
    const code = normalizePairingCode(pairingCode);
    if (
      !code ||
      typeof deviceId !== "string" ||
      typeof deviceToken !== "string"
    ) {
      return { ok: false, error: "bad_offer", status: 400 };
    }
    const existing = this.codes.get(code);
    if (existing) {
      if (
        existing.deviceId === deviceId &&
        tokensMatch(existing.token, deviceToken)
      ) {
        existing.displayName = displayName || existing.displayName;
        existing.expires = expiresUnix || existing.expires;
        return { ok: true, created: false, status: 200 };
      }
      return { ok: false, error: "taken", status: 409 };
    }
    this.codes.set(code, {
      deviceId,
      token: deviceToken,
      displayName: displayName || "Mac",
      expires: expiresUnix || this.nowSecs() + PAIRING_TTL_SECS,
    });
    return { ok: true, created: true, status: 200 };
  }

  dropOffer(rawCode) {
    const code = normalizePairingCode(rawCode);
    if (code) this.codes.delete(code);
    return { ok: true, status: 200 };
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
    this.codes.delete(code);
    const device = this.devices.get(offer.deviceId);
    const token = offer.token || device?.token;
    const displayName = offer.displayName || device?.displayName || "Mac";
    if (!token) {
      return { ok: false, error: "unknown_code", status: 404 };
    }
    return {
      ok: true,
      device_id: offer.deviceId,
      device_token: token,
      display_name: displayName,
      status: 200,
    };
  }

  heartbeat({
    deviceId,
    deviceToken,
    displayName,
    status,
    ackCommandIds,
    lang,
    origin,
  }) {
    const now = this.nowSecs();
    const device = this.devices.get(deviceId);
    if (!device || !tokensMatch(device.token, deviceToken)) {
      return { ok: false, error: "unauthorized", status: 401 };
    }
    if (displayName) device.displayName = displayName;
    if (status && typeof status === "object") {
      device.status = sanitizeStatus(status);
    }
    device.lastSeen = now;
    const acks = new Set(
      Array.isArray(ackCommandIds)
        ? ackCommandIds.filter((id) => typeof id === "string")
        : [],
    );
    device.commands = (device.commands || []).filter((item) => !acks.has(item.id));
    const pending = [...device.commands];
    const expiredCodes = this.#purgeCodes(now);
    let pairing = null;
    const chinese = isChineseLang(lang);
    for (const [code, offer] of this.codes) {
      if (offer.deviceId === deviceId) {
        pairing = {
          pairing_code: formatPairingCode(code),
          pairing_url: pairingUrl(code, chinese, origin),
        };
        break;
      }
    }
    return {
      ok: true,
      commands: pending,
      pairing_code: pairing?.pairing_code || null,
      pairing_url: pairing?.pairing_url || null,
      expired_codes: expiredCodes,
      status: 200,
    };
  }

  list(entriesIn) {
    if (!Array.isArray(entriesIn)) {
      return { ok: false, error: "bad_request", status: 400 };
    }
    const now = this.nowSecs();
    const devices = [];
    for (const entry of capListEntries(entriesIn)) {
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
        ...sanitizeStatus(device.status),
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
    if (cmd === "on" && duration != null && duration !== "") {
      if (typeof duration !== "string" || !isAllowedDuration(duration)) {
        return { ok: false, error: "bad_duration", status: 400 };
      }
      item.duration = duration.trim();
    }
    device.commands = device.commands || [];
    device.commands.push(item);
    return { ok: true, accepted: true, command_id: item.id, status: 200 };
  }
}

function requestOrigin(request, env = {}) {
  return (
    request.headers.get("x-public-origin") ||
    publicSiteOrigin(request.url, env)
  );
}

export async function handleInternal(board, request) {
  const url = new URL(request.url);
  const path = url.pathname.replace(/\/+$/, "") || "/";
  let body = {};
  try {
    body = await request.json();
  } catch {
    body = {};
  }
  let result;
  if (path === "/internal/pair-offer") {
    result = board.rememberOffer({
      pairingCode: body.pairing_code,
      deviceId: body.device_id,
      deviceToken: body.device_token,
      displayName: body.display_name,
      expiresUnix: body.expires_unix,
    });
  } else if (path === "/internal/pair-drop") {
    result = board.dropOffer(body.pairing_code);
  } else if (path === "/internal/pair-peek") {
    result = board.peekOffer(body.pairing_code);
  } else if (path === "/internal/live-pairing") {
    result = board.livePairing(body.device_id);
  } else {
    return jsonResponse({ ok: false, error: "not_found" }, 404);
  }
  const { status, ...payload } = result;
  return jsonResponse(payload, status);
}

export async function handleApi(board, request, env = {}) {
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
  const origin = requestOrigin(request, env);
  let result;
  switch (path) {
    case "/api/pair/start":
      result = board.startPairing({
        deviceId: body.device_id,
        deviceToken: body.device_token,
        displayName: body.display_name,
        lang: body.lang,
        origin,
        pairingCode: body.pairing_code,
        expiresUnix: body.expires_unix,
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
        ackCommandIds: body.ack_command_ids,
        lang: body.lang,
        origin,
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
