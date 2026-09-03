import assert from "node:assert/strict";
import test from "node:test";
import { Board, handleApi, HEARTBEAT_TTL_SECS } from "../src/board.js";

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
