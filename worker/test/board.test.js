import assert from "node:assert/strict";
import test from "node:test";
import {
  Board,
  handleApi,
  HEARTBEAT_TTL_SECS,
  pairingUrl,
  publicSiteOrigin,
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
