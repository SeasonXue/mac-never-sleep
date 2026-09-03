(() => {
  const STORAGE_KEY = "never-sleep-devices";
  const zh = document.documentElement.lang.startsWith("zh");

  const copy = zh
    ? {
        add: "添加 Mac",
        start: "开始关屏待命",
        end: "结束待命",
        online: "在线",
        offline: "离线",
        lastSeen: "上次见到",
        standbyOn: "待命中",
        standbyOff: "未待命",
        displayAsleep: "屏幕已关",
        displayAwake: "屏幕亮着",
        lidOpen: "开盖",
        lidClosed: "合盖",
        ac: "电源适配器",
        battery: "电池",
        remaining: "剩余",
        forget: "从本机列表移除",
        badCode: "配对码无效或已过期。",
        offlineCmd: "这台 Mac 当前离线，指令没有生效。",
        badCmd: "无法发送指令。",
        networkError: "网络出错，请检查连接后重试。",
        starting: "正在开始…",
        ending: "正在结束…",
      }
    : {
        add: "Add Mac",
        start: "Start Screen-Off Standby",
        end: "End Standby",
        online: "Online",
        offline: "Offline",
        lastSeen: "Last seen",
        standbyOn: "Standby on",
        standbyOff: "Standby off",
        displayAsleep: "Display asleep",
        displayAwake: "Display awake",
        lidOpen: "Lid open",
        lidClosed: "Lid closed",
        ac: "Power adapter",
        battery: "Battery",
        remaining: "left",
        forget: "Remove from this phone",
        badCode: "That pairing code is invalid or expired.",
        offlineCmd: "This Mac is offline; the command did not apply.",
        badCmd: "Could not send the command.",
        networkError: "Network error. Check the connection and try again.",
        starting: "Starting…",
        ending: "Ending…",
      };

  function apiBase() {
    const path = location.pathname;
    if (path.startsWith("/never-sleep/") || path.includes("/never-sleep/")) {
      return "/never-sleep/api";
    }
    return "/api";
  }

  function loadDevices() {
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      const parsed = raw ? JSON.parse(raw) : [];
      return Array.isArray(parsed) ? parsed : [];
    } catch {
      return [];
    }
  }

  function saveDevices(devices) {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(devices));
  }

  async function post(path, body) {
    const res = await fetch(`${apiBase()}${path}`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    });
    const json = await res.json().catch(() => ({}));
    return { res, json };
  }

  function formatLastSeen(unix) {
    if (!unix) return "—";
    const delta = Math.max(0, Math.floor(Date.now() / 1000) - unix);
    if (delta < 5) return zh ? "刚刚" : "just now";
    if (delta < 60) return zh ? `${delta} 秒前` : `${delta}s ago`;
    const min = Math.floor(delta / 60);
    if (min < 60) return zh ? `${min} 分钟前` : `${min}m ago`;
    const hr = Math.floor(min / 60);
    return zh ? `${hr} 小时前` : `${hr}h ago`;
  }

  function formatRemaining(secs) {
    if (secs == null) return "—";
    const s = Number(secs);
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    if (h > 0) return `${h}:${String(m).padStart(2, "0")}:00`;
    return `${m}:${String(s % 60).padStart(2, "0")}`;
  }

  function el(tag, className, text) {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text != null) node.textContent = text;
    return node;
  }

  function withListFailure(statuses) {
    return (statuses || []).map((st) => Object.assign({}, st, { online: false }));
  }

  function trustedView(stored, incoming, previous) {
    const src = incoming || {};
    const prev = previous || {};
    const from = incoming ? src : prev;
    return {
      device_id: stored.device_id,
      display_name:
        (typeof from.display_name === "string" && from.display_name) ||
        stored.display_name ||
        "Mac",
      online: incoming ? src.online === true : false,
      last_seen_unix:
        typeof from.last_seen_unix === "number" ? from.last_seen_unix : null,
      active: from.active === true,
      display: from.display === "asleep" ? "asleep" : "awake",
      lid: from.lid === "closed" ? "closed" : "open",
      on_ac: from.on_ac === true,
      battery: Number.isFinite(from.battery) ? from.battery : null,
      remaining_secs: Number.isFinite(from.remaining_secs)
        ? from.remaining_secs
        : null,
    };
  }

  function render(devices, statuses, pending) {
    const list = document.getElementById("device-list");
    const empty = document.getElementById("board-empty");
    list.replaceChildren();
    empty.hidden = devices.length > 0;
    const byId = new Map((statuses || []).map((d) => [d.device_id, d]));
    const prevById = new Map((lastStatuses || []).map((d) => [d.device_id, d]));
    for (const stored of devices) {
      const st = trustedView(
        stored,
        byId.get(stored.device_id),
        prevById.get(stored.device_id),
      );
      const card = el("article", "device-card" + (st.online ? "" : " offline"));
      const power = st.on_ac
        ? copy.ac
        : `${copy.battery}${st.battery != null ? ` ${st.battery}%` : ""}`;
      const remain =
        st.active && st.remaining_secs != null
          ? `${formatRemaining(st.remaining_secs)} ${copy.remaining}`
          : "—";
      const busy = pending === stored.device_id;

      const head = el("div", "device-head");
      head.appendChild(el("div", "device-name", st.display_name));
      head.appendChild(
        el(
          "div",
          "device-online" + (st.online ? "" : " off"),
          st.online ? copy.online : copy.offline,
        ),
      );
      card.appendChild(head);

      const meta = el("dl", "device-meta");
      const standby = el("div");
      standby.appendChild(
        el("strong", "", st.active ? copy.standbyOn : copy.standbyOff),
      );
      meta.appendChild(standby);
      meta.appendChild(
        el(
          "div",
          "",
          st.display === "asleep" ? copy.displayAsleep : copy.displayAwake,
        ),
      );
      meta.appendChild(
        el("div", "", st.lid === "closed" ? copy.lidClosed : copy.lidOpen),
      );
      meta.appendChild(el("div", "", power));
      meta.appendChild(el("div", "", remain));
      meta.appendChild(
        el("div", "", `${copy.lastSeen} ${formatLastSeen(st.last_seen_unix)}`),
      );
      card.appendChild(meta);

      const actions = el("div", "device-actions");
      const startBtn = el("button", "btn btn-primary", copy.start);
      startBtn.type = "button";
      startBtn.setAttribute("data-cmd", "on");
      const endBtn = el("button", "btn btn-danger", copy.end);
      endBtn.type = "button";
      endBtn.setAttribute("data-cmd", "off");
      if (!st.online || busy) {
        startBtn.disabled = true;
        endBtn.disabled = true;
      }
      if (busy && pendingCmd === "on") startBtn.textContent = copy.starting;
      if (busy && pendingCmd === "off") endBtn.textContent = copy.ending;
      startBtn.addEventListener("click", () => sendCommand(stored, "on"));
      endBtn.addEventListener("click", () => sendCommand(stored, "off"));
      actions.appendChild(startBtn);
      actions.appendChild(endBtn);
      card.appendChild(actions);

      const forget = el("button", "forget", copy.forget);
      forget.type = "button";
      forget.addEventListener("click", () => {
        saveDevices(
          loadDevices().filter((d) => d.device_id !== stored.device_id),
        );
        refresh();
      });
      card.appendChild(forget);
      list.appendChild(card);
    }
  }

  let pendingId = null;
  let pendingCmd = null;
  let lastStatuses = [];

  function showError(message) {
    const elErr = document.getElementById("board-error");
    if (!message) {
      elErr.hidden = true;
      elErr.textContent = "";
      return;
    }
    elErr.hidden = false;
    elErr.textContent = message;
  }

  async function refresh() {
    const devices = loadDevices();
    if (!devices.length) {
      render([], lastStatuses, null);
      return;
    }
    try {
      const { res, json } = await post("/list", { devices });
      if (!res.ok || !Array.isArray(json.devices)) {
        render(devices, withListFailure(lastStatuses), pendingId);
        return;
      }
      render(devices, json.devices, pendingId);
      lastStatuses = json.devices;
    } catch {
      render(devices, withListFailure(lastStatuses), pendingId);
    }
  }

  async function sendCommand(device, cmd) {
    showError("");
    pendingId = device.device_id;
    pendingCmd = cmd;
    render(loadDevices(), lastStatuses, pendingId);
    try {
      const { res, json } = await post("/command", {
        device_id: device.device_id,
        device_token: device.device_token,
        cmd,
      });
      if (res.status === 409 || json.error === "offline") {
        showError(copy.offlineCmd);
      } else if (!res.ok || json.ok === false) {
        showError(copy.badCmd);
      }
    } catch {
      showError(copy.networkError);
    }
    pendingId = null;
    pendingCmd = null;
    await refresh();
  }

  async function claim(code) {
    showError("");
    const input = document.getElementById("pair-code");
    try {
      const { res, json } = await post("/pair/claim", { pairing_code: code });
      if (!res.ok || !json.ok) {
        showError(copy.badCode);
        return;
      }
      const devices = loadDevices().filter((d) => d.device_id !== json.device_id);
      devices.push({
        device_id: json.device_id,
        device_token: json.device_token,
        display_name: json.display_name,
      });
      saveDevices(devices);
      input.value = "";
      await refresh();
    } catch {
      showError(copy.networkError);
    }
  }

  document.getElementById("pair-form").addEventListener("submit", (event) => {
    event.preventDefault();
    const code = document.getElementById("pair-code").value;
    if (code.trim()) claim(code);
  });

  const params = new URLSearchParams(location.search);
  const initial = params.get("code");
  if (initial) {
    document.getElementById("pair-code").value = initial;
    claim(initial);
  } else {
    refresh();
  }
  window.setInterval(refresh, 2500);
})();
