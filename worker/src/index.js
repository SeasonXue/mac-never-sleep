import {
  Board,
  handleApi,
  handleInternal,
  jsonResponse,
  normalizePairingCode,
  publicSiteOrigin,
  shardName,
} from "./board.js";

/**
 * One Durable Object per Mac (`device:{id}`) plus a short-lived pairing
 * shard (`pair:{code}`). Pair/start must not serialize every installation
 * through a single global object.
 */
export class BoardHub {
  constructor(ctx) {
    this.ctx = ctx;
  }

  async fetch(request) {
    const stored = (await this.ctx.storage.get("board")) || null;
    const board = Board.fromJSON(stored);
    board.nowSecs = () => Math.floor(Date.now() / 1000);
    const url = new URL(request.url);
    const path = url.pathname.replace(/\/+$/, "") || "/";
    const response = path.startsWith("/internal/")
      ? await handleInternal(board, request)
      : await handleApi(board, request);
    await this.ctx.storage.put("board", board.toJSON());
    return response;
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
    const entries = Array.isArray(body.devices) ? body.devices : [];
    const devices = [];
    const parts = await Promise.all(
      entries.map(async (entry) => {
        if (typeof entry?.device_id !== "string" || entry.device_id.length < 16) {
          return [];
        }
        const res = await stubFetch(
          env,
          `device:${entry.device_id}`,
          url.href,
          { devices: [entry] },
          origin,
        );
        const json = await res.json().catch(() => ({}));
        return Array.isArray(json.devices) ? json.devices : [];
      }),
    );
    for (const part of parts) devices.push(...part);
    return jsonResponse({ ok: true, devices });
  }

  if (path === "/api/pair/claim") {
    const name = shardName(path, body);
    if (!name) {
      return jsonResponse({ ok: false, error: "unknown_code" }, 404);
    }
    const res = await stubFetch(env, name, url.href, body, origin);
    const json = await res.clone().json().catch(() => ({}));
    if (json.ok && json.device_id) {
      await stubFetch(
        env,
        `device:${json.device_id}`,
        "https://do/internal/pair-drop",
        { pairing_code: body.pairing_code },
        origin,
      );
    }
    return res;
  }

  const name = shardName(path, body);
  if (!name) {
    return jsonResponse({ ok: false, error: "bad_identity" }, 400);
  }
  const res = await stubFetch(env, name, url.href, body, origin);
  if (path === "/api/pair/start") {
    const json = await res.clone().json().catch(() => ({}));
    if (json.ok && json.pairing_code) {
      await dropPairShards(env, json.replaced_codes, origin);
      const code = normalizePairingCode(json.pairing_code);
      if (code) {
        await stubFetch(
          env,
          `pair:${code}`,
          "https://do/internal/pair-offer",
          {
            pairing_code: json.pairing_code,
            device_id: body.device_id,
            device_token: body.device_token,
            display_name: body.display_name,
            expires_unix: json.expires_unix,
          },
          origin,
        );
      }
    }
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
