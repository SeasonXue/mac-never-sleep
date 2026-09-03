import {
  Board,
  capListEntries,
  collectListParts,
  createSerialQueue,
  formatPairingCode,
  handleApi,
  handleInternal,
  jsonResponse,
  normalizePairingCode,
  PAIRING_TTL_SECS,
  persistBoardAction,
  proposePairingCode,
  publicSiteOrigin,
  clientIp,
  publishReservedPairing,
  shardName,
  bestEffortCleanup,
  commitPersistedAlarm,
  pairingCodeIsLive,
} from "./board.js";

/**
 * One Durable Object per Mac (`device:{id}`) plus a short-lived pairing
 * shard (`pair:{code}`). Pair/start must not serialize every installation
 * through a single global object.
 */
export class BoardHub {
  constructor(ctx) {
    this.ctx = ctx;
    this.stored = null;
    this.board = new Board();
    this.enqueue = createSerialQueue();
    this.alarmUnix = undefined;
    ctx.blockConcurrencyWhile(async () => {
      this.stored = (await ctx.storage.get("board")) || null;
      this.board = Board.fromJSON(this.stored);
      await this.#scheduleAlarm();
    });
  }

  fetch(request) {
    return this.enqueue(() => this.#apply(request));
  }

  alarm() {
    return this.enqueue(() => this.#onAlarm());
  }

  async #apply(request) {
    this.board.nowSecs = () => Math.floor(Date.now() / 1000);
    const url = new URL(request.url);
    const path = url.pathname.replace(/\/+$/, "") || "/";
    const response = path.startsWith("/internal/")
      ? await handleInternal(this.board, request)
      : await handleApi(this.board, request);
    await this.#persist();
    return response;
  }

  async #onAlarm() {
    this.alarmUnix = undefined;
    this.board.nowSecs = () => Math.floor(Date.now() / 1000);
    this.board.expireOffers();
    await this.#persist();
  }

  async #persist() {
    const next = this.board.toJSON();
    const action = persistBoardAction(this.stored, next);
    try {
      if (action === "put") {
        await this.ctx.storage.put("board", next);
        this.stored = next;
      } else if (action === "delete") {
        await this.ctx.storage.delete("board");
        this.stored = null;
        this.board = Board.fromJSON(null);
      }
    } catch (err) {
      this.board = Board.fromJSON(this.stored);
      throw err;
    }
    await this.#scheduleAlarm();
  }

  async #scheduleAlarm() {
    const next = this.board.nextAlarmUnix();
    this.alarmUnix = await commitPersistedAlarm(this.alarmUnix, next, async (unix) => {
      if (unix == null) {
        await this.ctx.storage.deleteAlarm();
      } else {
        await this.ctx.storage.setAlarm(unix * 1000);
      }
    });
  }
}

function jsonRequest(url, body, origin) {
  return new Request(url, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      "x-public-origin": origin,
    },
    body: JSON.stringify(body),
  });
}

async function stubFetch(env, name, url, body, origin) {
  const id = env.BOARD.idFromName(name);
  return env.BOARD.get(id).fetch(jsonRequest(url, body, origin));
}

async function dropPairShards(env, codes, origin) {
  await Promise.all(
    (codes || []).map((code) => {
      const normalized = normalizePairingCode(code) || code;
      return stubFetch(
        env,
        `pair:${normalized}`,
        "https://do/internal/pair-drop",
        { pairing_code: code },
        origin,
      );
    }),
  );
}

/**
 * @param {Request} request
 * @param {{ BOARD: DurableObjectNamespace, ASSETS?: { fetch: typeof fetch }, PUBLIC_SITE_ORIGIN?: string, NEVER_SLEEP_CLOUD_URL?: string }} env
 */
async function routeApi(request, env) {
  const url = new URL(request.url);
  const path = url.pathname.replace(/\/+$/, "") || "/";
  const origin = publicSiteOrigin(request.url, env);
  let body = {};
  try {
    body = await request.json();
  } catch {
    body = {};
  }

  if (path === "/api/list") {
    const entries = capListEntries(body.devices);
    const devices = await collectListParts(async (entry) => {
      const res = await stubFetch(
        env,
        `device:${entry.device_id}`,
        url.href,
        { devices: [entry] },
        origin,
      );
      const json = await res.json().catch(() => ({}));
      return Array.isArray(json.devices) ? json.devices : [];
    }, entries);
    return jsonResponse({ ok: true, devices });
  }

  if (path === "/api/pair/claim") {
    const name = shardName(path, body);
    if (!name) {
      return jsonResponse({ ok: false, error: "unknown_code" }, 404);
    }
    const peek = await stubFetch(
      env,
      name,
      "https://do/internal/pair-peek",
      { pairing_code: body.pairing_code },
      origin,
    );
    const peeked = await peek.json().catch(() => ({}));
    if (!peeked.ok || !peeked.device_id) {
      return jsonResponse({ ok: false, error: "unknown_code" }, 404);
    }
    const res = await stubFetch(
      env,
      `device:${peeked.device_id}`,
      url.href,
      body,
      origin,
    );
    return bestEffortCleanup(res, () =>
      dropPairShards(env, [body.pairing_code], origin),
    );
  }

  if (path === "/api/pair/start") {
    const name = shardName(path, body);
    if (!name) {
      return jsonResponse({ ok: false, error: "bad_identity" }, 400);
    }
    const gated = await stubFetch(
      env,
      "rate:pair-start",
      "https://do/internal/pair-rate",
      { ip: clientIp(request) },
      origin,
    );
    const gate = await gated.json().catch(() => ({}));
    if (!gate.ok) {
      return jsonResponse(
        { ok: false, error: gate.error || "rate_limited" },
        gate.status || 429,
      );
    }
    const expiresUnix = Math.floor(Date.now() / 1000) + PAIRING_TTL_SECS;
    const started = await publishReservedPairing({
      generateCode: proposePairingCode,
      reserve: async (code) => {
        const res = await stubFetch(
          env,
          `pair:${code}`,
          "https://do/internal/pair-offer",
          {
            pairing_code: formatPairingCode(code),
            device_id: body.device_id,
            device_token: body.device_token,
            display_name: body.display_name,
            expires_unix: expiresUnix,
          },
          origin,
        );
        return res.json().catch(() => ({}));
      },
      startDevice: async (code) => {
        const res = await stubFetch(
          env,
          name,
          url.href,
          {
            ...body,
            pairing_code: formatPairingCode(code),
            expires_unix: expiresUnix,
          },
          origin,
        );
        const json = await res.json().catch(() => ({}));
        return { ...json, status: res.status };
      },
      release: async (code) => {
        await dropPairShards(env, [code], origin);
      },
      confirmLive: async (code) => {
        const live = await stubFetch(
          env,
          name,
          "https://do/internal/live-pairing",
          { device_id: body.device_id },
          origin,
        );
        const json = await live.json().catch(() => ({}));
        return pairingCodeIsLive(json.pairing_code, formatPairingCode(code));
      },
    });
    if (started.ok) {
      const { status, ...payload } = started;
      const res = jsonResponse(payload, status || 200);
      return bestEffortCleanup(res, () =>
        dropPairShards(env, started.replaced_codes, origin),
      );
    }
    return jsonResponse(
      { ok: false, error: started.error || "pair_busy" },
      started.status || 503,
    );
  }

  const name = shardName(path, body);
  if (!name) {
    return jsonResponse({ ok: false, error: "bad_identity" }, 400);
  }
  const res = await stubFetch(env, name, url.href, body, origin);
  if (path === "/api/heartbeat") {
    const json = await res.clone().json().catch(() => ({}));
    await dropPairShards(env, json.expired_codes, origin);
  }
  return res;
}

export default {
  /**
   * @param {Request} request
   * @param {{ BOARD: DurableObjectNamespace, ASSETS?: { fetch: typeof fetch } }} env
   */
  async fetch(request, env) {
    const url = new URL(request.url);
    if (url.pathname === "/api" || url.pathname.startsWith("/api/")) {
      return routeApi(request, env);
    }
    if (env.ASSETS) {
      return env.ASSETS.fetch(request);
    }
    return new Response("Not found", { status: 404 });
  },
};
