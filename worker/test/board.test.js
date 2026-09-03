import assert from "node:assert/strict";
import test from "node:test";
import {
  Board,
  capListEntries,
  handleApi,
  HEARTBEAT_TTL_SECS,
  isAllowedDuration,
  LIST_MAX_DEVICES,
  PAIRING_TTL_SECS,
  pairingCodeIsLive,
  pairingUrl,
  persistBoardAction,
  publicSiteOrigin,
  publishReservedPairing,
  sanitizeStatus,
  shardName,
} from "../src/board.js";

function sampleStatus(overrides = {}) {
  return {
    active: false,
    display: "awake",
    lid: "open",
    on_ac: true,
    battery: 64,
    remaining_secs: null,
    user_present: true,
    elapsed_secs: null,
    stop_reason: null,
    stop_reason_code: null,
    screen_off_enabled: true,
    lid_awake_enabled: true,
    ...overrides,
  };
}

function identity() {
  return {
    device_id: "ab".repeat(16),
    device_token: "cd".repeat(32),
    display_name: "Studio",
  };
}

async function post(board, path, body) {
  return handleApi(
    board,
    new Request(`https://xyz-ai.app${path}`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    }),
  );
}

async function json(res) {
  return { status: res.status, body: await res.json() };
}

test("pairing creates a device the list endpoint returns", async () => {
  const board = new Board(() => 1_000);
  const id = identity();
  const started = await json(
    await post(board, "/api/pair/start", {
      device_id: id.device_id,
      device_token: id.device_token,
      display_name: id.display_name,
    }),
  );
  assert.equal(started.status, 200);
  assert.equal(started.body.ok, true);
  assert.match(started.body.pairing_code, /^[0-9A-Z]{4}-[0-9A-Z]{4}$/);
  assert.ok(started.body.pairing_url.includes("/board/"));

  const claimed = await json(
    await post(board, "/api/pair/claim", {
      pairing_code: started.body.pairing_code,
    }),
  );
  assert.equal(claimed.status, 200);
  assert.equal(claimed.body.device_id, id.device_id);
  assert.equal(claimed.body.device_token, id.device_token);

  const listed = await json(
    await post(board, "/api/list", {
      devices: [
        { device_id: id.device_id, device_token: id.device_token },
      ],
    }),
  );
  assert.equal(listed.body.devices.length, 1);
  assert.equal(listed.body.devices[0].display_name, "Studio");
  assert.equal(listed.body.devices[0].device_id, id.device_id);
  assert.equal(listed.body.devices[0].online, false);
});

test("heartbeat auth rejects bad tokens", async () => {
  const board = new Board(() => 1_000);
  const id = identity();
  await post(board, "/api/pair/start", id);
  const bad = await json(
    await post(board, "/api/heartbeat", {
      device_id: id.device_id,
      device_token: "ff".repeat(32),
      status: sampleStatus(),
    }),
  );
  assert.equal(bad.status, 401);
  assert.equal(bad.body.ok, false);
  const good = await json(
    await post(board, "/api/heartbeat", {
      device_id: id.device_id,
      device_token: id.device_token,
      display_name: "Studio",
      status: sampleStatus({ active: true, display: "asleep" }),
    }),
  );
  assert.equal(good.status, 200);
  assert.equal(good.body.ok, true);
});

test("offline after heartbeat TTL", async () => {
  let now = 1_000;
  const board = new Board(() => now);
  const id = identity();
  await post(board, "/api/pair/start", id);
  await post(board, "/api/heartbeat", {
    device_id: id.device_id,
    device_token: id.device_token,
    status: sampleStatus({ active: true }),
  });
  now = 1_000 + HEARTBEAT_TTL_SECS;
  let listed = await json(
    await post(board, "/api/list", {
      devices: [{ device_id: id.device_id, device_token: id.device_token }],
    }),
  );
  assert.equal(listed.body.devices[0].online, true);
  assert.equal(listed.body.devices[0].active, true);
  now = 1_000 + HEARTBEAT_TTL_SECS + 1;
  listed = await json(
    await post(board, "/api/list", {
      devices: [{ device_id: id.device_id, device_token: id.device_token }],
    }),
  );
  assert.equal(listed.body.devices[0].online, false);
  assert.equal(
    listed.body.devices[0].active,
    true,
    "last status is retained for last-seen; the phone must key off online",
  );
});

test("authorized on/off queues a command; bad token is rejected", async () => {
  const board = new Board(() => 1_000);
  const id = identity();
  await post(board, "/api/pair/start", id);
  await post(board, "/api/heartbeat", {
    device_id: id.device_id,
    device_token: id.device_token,
    status: sampleStatus(),
  });
  const denied = await json(
    await post(board, "/api/command", {
      device_id: id.device_id,
      device_token: "00".repeat(32),
      cmd: "on",
    }),
  );
  assert.equal(denied.status, 401);

  const on = await json(
    await post(board, "/api/command", {
      device_id: id.device_id,
      device_token: id.device_token,
      cmd: "on",
      duration: "8h",
    }),
  );
  assert.equal(on.status, 200);
  assert.equal(on.body.accepted, true);

  const beat = await json(
    await post(board, "/api/heartbeat", {
      device_id: id.device_id,
      device_token: id.device_token,
      status: sampleStatus(),
    }),
  );
  assert.equal(beat.body.commands.length, 1);
  assert.equal(beat.body.commands[0].cmd, "on");
  assert.equal(beat.body.commands[0].duration, "8h");

  const off = await json(
    await post(board, "/api/command", {
      device_id: id.device_id,
      device_token: id.device_token,
      cmd: "off",
    }),
  );
  assert.equal(off.body.accepted, true);
});

test("offline Mac does not report command success", async () => {
  let now = 1_000;
  const board = new Board(() => now);
  const id = identity();
  await post(board, "/api/pair/start", id);
  now = 1_000 + HEARTBEAT_TTL_SECS + 5;
  const res = await json(
    await post(board, "/api/command", {
      device_id: id.device_id,
      device_token: id.device_token,
      cmd: "on",
    }),
  );
  assert.equal(res.status, 409);
  assert.equal(res.body.ok, false);
  assert.equal(res.body.error, "offline");
  assert.equal(res.body.accepted, false);
});

test("network path rejects toggle/quit and does not leak other devices", async () => {
  const board = new Board(() => 1_000);
  const a = identity();
  const b = {
    device_id: "11".repeat(16),
    device_token: "22".repeat(32),
    display_name: "Kitchen",
  };
  await post(board, "/api/pair/start", a);
  await post(board, "/api/pair/start", b);
  await post(board, "/api/heartbeat", {
    device_id: a.device_id,
    device_token: a.device_token,
    status: sampleStatus(),
  });
  const toggle = await json(
    await post(board, "/api/command", {
      device_id: a.device_id,
      device_token: a.device_token,
      cmd: "toggle",
    }),
  );
  assert.equal(toggle.status, 400);
  const quit = await json(
    await post(board, "/api/command", {
      device_id: a.device_id,
      device_token: a.device_token,
      cmd: "quit",
    }),
  );
  assert.equal(quit.status, 400);

  const listed = await json(
    await post(board, "/api/list", {
      devices: [
        { device_id: a.device_id, device_token: a.device_token },
        { device_id: b.device_id, device_token: a.device_token },
      ],
    }),
  );
  assert.equal(listed.body.devices.length, 1);
  assert.equal(listed.body.devices[0].device_id, a.device_id);
  assert.equal(
    listed.body.devices[0].display_name,
    "Studio",
    "wrong token must not reveal Kitchen",
  );
});

test("pairing code is one-time: second claim is 404", async () => {
  const board = new Board(() => 1_000);
  const id = identity();
  const started = await json(await post(board, "/api/pair/start", id));
  const first = await json(
    await post(board, "/api/pair/claim", {
      pairing_code: started.body.pairing_code,
    }),
  );
  assert.equal(first.status, 200);
  assert.equal(first.body.device_token, id.device_token);
  const second = await json(
    await post(board, "/api/pair/claim", {
      pairing_code: started.body.pairing_code,
    }),
  );
  assert.equal(second.status, 404);
  assert.equal(second.body.ok, false);
  assert.equal(second.body.error, "unknown_code");
});

test("commands stay queued until the Mac acks receipt", async () => {
  const board = new Board(() => 1_000);
  const id = identity();
  await post(board, "/api/pair/start", id);
  await post(board, "/api/heartbeat", {
    device_id: id.device_id,
    device_token: id.device_token,
    status: sampleStatus(),
  });
  const on = await json(
    await post(board, "/api/command", {
      device_id: id.device_id,
      device_token: id.device_token,
      cmd: "on",
    }),
  );
  assert.equal(on.body.accepted, true);
  const commandId = on.body.command_id;
  const first = await json(
    await post(board, "/api/heartbeat", {
      device_id: id.device_id,
      device_token: id.device_token,
      status: sampleStatus(),
    }),
  );
  assert.equal(first.body.commands.length, 1);
  assert.equal(first.body.commands[0].id, commandId);
  const lostRetry = await json(
    await post(board, "/api/heartbeat", {
      device_id: id.device_id,
      device_token: id.device_token,
      status: sampleStatus(),
    }),
  );
  assert.equal(
    lostRetry.body.commands.length,
    1,
    "a lost heartbeat response must not drop the command",
  );
  const acked = await json(
    await post(board, "/api/heartbeat", {
      device_id: id.device_id,
      device_token: id.device_token,
      status: sampleStatus(),
      ack_command_ids: [commandId],
    }),
  );
  assert.equal(acked.body.commands.length, 0);
  const afterAck = await json(
    await post(board, "/api/heartbeat", {
      device_id: id.device_id,
      device_token: id.device_token,
      status: sampleStatus(),
    }),
  );
  assert.equal(afterAck.body.commands.length, 0);
});

test("pairing URL uses the serving origin, not a hard-coded production host", async () => {
  const board = new Board(() => 1_000);
  const id = identity();
  const res = await handleApi(
    board,
    new Request("http://127.0.0.1:8787/api/pair/start", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(id),
    }),
  );
  const body = await res.json();
  assert.equal(res.status, 200);
  assert.ok(
    body.pairing_url.startsWith("http://127.0.0.1:8787/board/"),
    body.pairing_url,
  );
  assert.ok(!body.pairing_url.includes("xyz-ai.app"));
  assert.equal(
    publicSiteOrigin("https://xyz-ai.app/api/pair/start"),
    "https://xyz-ai.app/never-sleep",
  );
  assert.equal(
    publicSiteOrigin("https://mac-never-sleep.example.workers.dev/api/x"),
    "https://mac-never-sleep.example.workers.dev",
  );
  assert.equal(
    publicSiteOrigin("http://127.0.0.1:8787/api/x", {
      NEVER_SLEEP_CLOUD_URL: "https://preview.example/never-sleep",
    }),
    "https://preview.example/never-sleep",
  );
  assert.equal(
    pairingUrl("AB7K2Q9M", true, "http://127.0.0.1:8787").includes("/zh/board/"),
    true,
  );
});

test("Chinese lang on pair/start and heartbeat returns /zh/board/", async () => {
  const board = new Board(() => 1_000);
  const id = identity();
  const started = await json(
    await post(board, "/api/pair/start", { ...id, lang: "zh" }),
  );
  assert.ok(started.body.pairing_url.includes("/zh/board/"));
  const beat = await json(
    await post(board, "/api/heartbeat", {
      device_id: id.device_id,
      device_token: id.device_token,
      lang: "zh",
      status: sampleStatus(),
    }),
  );
  assert.ok(beat.body.pairing_url.includes("/zh/board/"));
});

test("heartbeat sanitizes untrusted status before it can reach the phone", async () => {
  const board = new Board(() => 1_000);
  const id = identity();
  await post(board, "/api/pair/start", id);
  await post(board, "/api/heartbeat", {
    device_id: id.device_id,
    device_token: id.device_token,
    status: {
      active: true,
      display: "<script>alert(1)</script>",
      lid: "closed<img>",
      on_ac: "yes",
      battery: "<img src=x onerror=alert(1)>",
      remaining_secs: "1e999",
      user_present: 1,
      elapsed_secs: { steal: true },
      stop_reason: "<img src=x onerror=document.location=localStorage>",
      stop_reason_code: "user<script>",
      screen_off_enabled: "true",
      lid_awake_enabled: 2,
    },
  });
  const listed = await json(
    await post(board, "/api/list", {
      devices: [{ device_id: id.device_id, device_token: id.device_token }],
    }),
  );
  const st = listed.body.devices[0];
  assert.equal(st.display, "awake");
  assert.equal(st.lid, "open");
  assert.equal(st.on_ac, false);
  assert.equal(st.battery, null);
  assert.equal(st.remaining_secs, null);
  assert.equal(st.user_present, false);
  assert.equal(st.elapsed_secs, null);
  assert.equal(st.stop_reason, null);
  assert.equal(st.stop_reason_code, null);
  assert.equal(st.screen_off_enabled, false);
  assert.equal(st.lid_awake_enabled, false);
  assert.equal(st.active, true);
  const xss = sanitizeStatus({
    battery: "<script>",
    display: "asleep",
    lid: "closed",
    on_ac: true,
    active: false,
    user_present: false,
    screen_off_enabled: true,
    lid_awake_enabled: true,
  });
  assert.equal(xss.battery, null);
  assert.equal(xss.display, "asleep");
});

test("API traffic is sharded per device and pairing code, never a global board", () => {
  const id = identity();
  assert.equal(shardName("/api/pair/start", id), `device:${id.device_id}`);
  assert.equal(shardName("/api/heartbeat", id), `device:${id.device_id}`);
  assert.equal(shardName("/api/command", id), `device:${id.device_id}`);
  assert.equal(
    shardName("/api/pair/claim", { pairing_code: "AB7K-2Q9M" }),
    "pair:AB7K2Q9M",
  );
  assert.equal(shardName("/api/list", { devices: [id] }), null);
  assert.notEqual(shardName("/api/pair/start", id), "board");
  assert.notEqual(shardName("/api/heartbeat", id), "board");
});

test("expired pairing codes are returned so the router can drop pair shards", async () => {
  let now = 1_000;
  const board = new Board(() => now);
  const id = identity();
  const started = await json(await post(board, "/api/pair/start", id));
  const raw = started.body.pairing_code.replace("-", "");
  now = 1_000 + PAIRING_TTL_SECS + 1;
  const beat = await json(
    await post(board, "/api/heartbeat", {
      device_id: id.device_id,
      device_token: id.device_token,
      status: sampleStatus(),
    }),
  );
  assert.equal(beat.body.pairing_code, null);
  assert.ok(
    beat.body.expired_codes.includes(raw),
    "heartbeat must report the expired offer so pair:{code} can be deleted",
  );
  const renewed = await json(await post(board, "/api/pair/start", id));
  assert.equal(renewed.status, 200);
  assert.notEqual(renewed.body.pairing_code, started.body.pairing_code);
});

test("startPairing after TTL lists the expired code as replaced", async () => {
  let now = 1_000;
  const board = new Board(() => now);
  const id = identity();
  const first = await json(await post(board, "/api/pair/start", id));
  const raw = first.body.pairing_code.replace("-", "");
  now = 1_000 + PAIRING_TTL_SECS + 1;
  const second = await json(await post(board, "/api/pair/start", id));
  assert.ok(
    second.body.replaced_codes.includes(raw),
    "router must be told to drop the expired pair:{code} shard",
  );
});

test("startPairing lists the previous live code as replaced", async () => {
  const board = new Board(() => 1_000);
  const id = identity();
  const first = await json(await post(board, "/api/pair/start", id));
  const raw = first.body.pairing_code.replace("-", "");
  const second = await json(await post(board, "/api/pair/start", id));
  assert.ok(second.body.replaced_codes.includes(raw));
  assert.notEqual(second.body.pairing_code, first.body.pairing_code);
});

test("stale pair/start must not publish after a newer code is live", () => {
  assert.equal(pairingCodeIsLive("ZZZZ-YYYY", "ZZZZ-YYYY"), true);
  assert.equal(
    pairingCodeIsLive("ZZZZ-YYYY", "AB7K-2Q9M"),
    false,
    "a superseded start must not resurrect its pair shard",
  );
  assert.equal(pairingCodeIsLive(null, "AB7K-2Q9M"), false);
  const board = new Board(() => 1_000);
  const id = identity();
  board.startPairing({
    deviceId: id.device_id,
    deviceToken: id.device_token,
    displayName: id.display_name,
  });
  const live = board.livePairing(id.device_id);
  const stale = board.startPairing({
    deviceId: id.device_id,
    deviceToken: id.device_token,
    displayName: id.display_name,
  });
  const newest = board.livePairing(id.device_id);
  assert.equal(
    pairingCodeIsLive(newest.pairing_code, live.pairing_code),
    false,
  );
  assert.equal(
    pairingCodeIsLive(newest.pairing_code, stale.pairing_code),
    true,
  );
});

test("list entries are capped and deduplicated before fan-out", () => {
  const a = { device_id: "aa".repeat(16), device_token: "t" };
  const b = { device_id: "bb".repeat(16), device_token: "t" };
  const capped = capListEntries([a, a, b, { device_id: "x" }]);
  assert.equal(capped.length, 2);
  assert.equal(capped[0].device_id, a.device_id);
  assert.equal(capped[1].device_id, b.device_id);
  const many = Array.from({ length: LIST_MAX_DEVICES + 10 }, (_, i) => ({
    device_id: String(i).padStart(32, "0"),
    device_token: "t",
  }));
  assert.equal(capListEntries(many).length, LIST_MAX_DEVICES);
});

test("empty lookup boards are not persisted", () => {
  assert.equal(persistBoardAction(null, { devices: {}, codes: {} }), "skip");
  assert.equal(
    persistBoardAction(null, { devices: { ab: { token: "x" } }, codes: {} }),
    "put",
  );
  assert.equal(
    persistBoardAction({ devices: { ab: {} }, codes: {} }, { devices: {}, codes: {} }),
    "delete",
  );
  assert.equal(
    persistBoardAction({ devices: {}, codes: {} }, { devices: {}, codes: {} }),
    "delete",
    "leftover empty boards from failed lookups are dropped",
  );
});

test("command duration must be a Rust-parseable string", async () => {
  const board = new Board(() => 1_000);
  const id = identity();
  await post(board, "/api/pair/start", id);
  await post(board, "/api/heartbeat", {
    device_id: id.device_id,
    device_token: id.device_token,
    status: sampleStatus(),
  });
  for (const duration of [8, { hours: 8 }, true, "nope", "0h"]) {
    const res = await json(
      await post(board, "/api/command", {
        device_id: id.device_id,
        device_token: id.device_token,
        cmd: "on",
        duration,
      }),
    );
    assert.equal(res.status, 400, JSON.stringify(duration));
    assert.equal(res.body.error, "bad_duration");
  }
  const emptyBeat = await json(
    await post(board, "/api/heartbeat", {
      device_id: id.device_id,
      device_token: id.device_token,
      status: sampleStatus(),
    }),
  );
  assert.equal(emptyBeat.body.commands.length, 0);
  const ok = await json(
    await post(board, "/api/command", {
      device_id: id.device_id,
      device_token: id.device_token,
      cmd: "on",
      duration: "240h",
    }),
  );
  assert.equal(ok.status, 200);
  const beat = await json(
    await post(board, "/api/heartbeat", {
      device_id: id.device_id,
      device_token: id.device_token,
      status: sampleStatus(),
    }),
  );
  assert.equal(beat.body.commands[0].duration, "240h");
});

test("sanitizeStatus keeps week-plus remaining and elapsed seconds", () => {
  const st = sanitizeStatus({
    active: true,
    display: "asleep",
    lid: "open",
    on_ac: true,
    remaining_secs: 240 * 3600,
    elapsed_secs: 8 * 24 * 3600,
    user_present: false,
    screen_off_enabled: true,
    lid_awake_enabled: true,
  });
  assert.equal(st.remaining_secs, 240 * 3600);
  assert.equal(st.elapsed_secs, 8 * 24 * 3600);
  const maxHours = sanitizeStatus({
    remaining_secs: 0xffffffff * 3600,
    elapsed_secs: Number.MAX_SAFE_INTEGER,
  });
  assert.equal(maxHours.remaining_secs, 0xffffffff * 3600);
  assert.equal(maxHours.elapsed_secs, Number.MAX_SAFE_INTEGER);
});

test("isAllowedDuration matches the Rust duration parser", () => {
  for (const raw of ["240h", "indefinite", "until=08:00", "22:30", "3小时"]) {
    assert.equal(isAllowedDuration(raw), true, raw);
  }
  assert.equal(isAllowedDuration("4294967295h"), true);
  assert.equal(isAllowedDuration("4294967296h"), false);
  assert.equal(isAllowedDuration(8), false);
  assert.equal(isAllowedDuration({ hours: 8 }), false);
});

test("pair/start uses a reserved pairing code when provided", async () => {
  const board = new Board(() => 1_000);
  const started = await json(
    await post(board, "/api/pair/start", {
      ...identity(),
      pairing_code: "AB7K-2Q9M",
      expires_unix: 2_000,
    }),
  );
  assert.equal(started.status, 200);
  assert.equal(started.body.pairing_code, "AB7K-2Q9M");
  assert.equal(started.body.expires_unix, 2000);
});

test("pair shard reservation is create-if-absent and retries on collision", async () => {
  const board = new Board(() => 1_000);
  const a = identity();
  const b = {
    device_id: "11".repeat(16),
    device_token: "22".repeat(32),
    display_name: "Kitchen",
  };
  const first = board.rememberOffer({
    pairingCode: "AB7K2Q9M",
    deviceId: a.device_id,
    deviceToken: a.device_token,
    displayName: a.display_name,
  });
  assert.equal(first.ok, true);
  const clash = board.rememberOffer({
    pairingCode: "AB7K2Q9M",
    deviceId: b.device_id,
    deviceToken: b.device_token,
    displayName: b.display_name,
  });
  assert.equal(clash.ok, false);
  assert.equal(clash.status, 409);
  const codes = ["AB7K2Q9M", "ZZZZYYYY"];
  const result = await publishReservedPairing({
    generateCode: () => codes.shift(),
    reserve: async (code) =>
      code === "AB7K2Q9M" ? { ok: false, error: "taken" } : { ok: true },
    startDevice: async (code) => ({ ok: true, pairing_code: code }),
  });
  assert.equal(result.pairing_code, "ZZZZYYYY");
});
