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

  function render(devices, statuses, pending) {
    const list = document.getElementById("device-list");
    const empty = document.getElementById("board-empty");
    list.innerHTML = "";
    empty.hidden = devices.length > 0;
    const byId = new Map((statuses || []).map((d) => [d.device_id, d]));
    for (const stored of devices) {
      const st = byId.get(stored.device_id) || {
        device_id: stored.device_id,
        display_name: stored.display_name,
        online: false,
        last_seen_unix: null,
        active: false,
        display: "awake",
        lid: "open",
        on_ac: true,
        battery: null,
        remaining_secs: null,
      };
      const card = document.createElement("article");
      card.className = "device-card" + (st.online ? "" : " offline");
      const power = st.on_ac
        ? copy.ac
        : `${copy.battery}${st.battery != null ? ` ${st.battery}%` : ""}`;
      const remain =
        st.active && st.remaining_secs != null
          ? `${formatRemaining(st.remaining_secs)} ${copy.remaining}`
          : "—";
      const busy = pending === stored.device_id;
      card.innerHTML = `
        <div class="device-head">
          <div class="device-name"></div>
          <div class="device-online ${st.online ? "" : "off"}">${
            st.online ? copy.online : copy.offline
          }</div>
        </div>
        <dl class="device-meta">
          <div><strong>${
            st.online && st.active ? copy.standbyOn : copy.standbyOff
          }</strong></div>
          <div>${st.display === "asleep" ? copy.displayAsleep : copy.displayAwake}</div>
          <div>${st.lid === "closed" ? copy.lidClosed : copy.lidOpen}</div>
          <div>${power}</div>
          <div>${remain}</div>
          <div>${copy.lastSeen} ${formatLastSeen(st.last_seen_unix)}</div>
        </dl>
        <div class="device-actions">
          <button class="btn btn-primary" data-cmd="on" type="button" ${
            !st.online || busy ? "disabled" : ""
          }>${busy && pendingCmd === "on" ? copy.starting : copy.start}</button>
          <button class="btn btn-danger" data-cmd="off" type="button" ${
            !st.online || busy ? "disabled" : ""
          }>${busy && pendingCmd === "off" ? copy.ending : copy.end}</button>
        </div>
        <button class="forget" type="button" data-forget>${copy.forget}</button>
      `;
      card.querySelector(".device-name").textContent =
        st.display_name || stored.display_name || "Mac";
      card.querySelectorAll("[data-cmd]").forEach((btn) => {
        btn.addEventListener("click", () => sendCommand(stored, btn.getAttribute("data-cmd")));
      });
      card.querySelector("[data-forget]").addEventListener("click", () => {
        saveDevices(loadDevices().filter((d) => d.device_id !== stored.device_id));
        refresh();
      });
      list.appendChild(card);
    }
  }

  let pendingId = null;
  let pendingCmd = null;

  function showError(message) {
    const el = document.getElementById("board-error");
    if (!message) {
      el.hidden = true;
      el.textContent = "";
      return;
    }
    el.hidden = false;
    el.textContent = message;
  }

  async function refresh() {
    const devices = loadDevices();
    if (!devices.length) {
      render([], [], null);
      return;
    }
    try {
      const { json } = await post("/list", { devices });
      render(devices, json.devices || [], pendingId);
    } catch {
      render(devices, [], pendingId);
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
      showError(copy.badCmd);
    }
    pendingId = null;
    pendingCmd = null;
    await refresh();
  }

  let lastStatuses = [];
  const _render = render;
  render = function (devices, statuses, pending) {
    lastStatuses = statuses || lastStatuses;
    _render(devices, statuses, pending);
  };

  async function claim(code) {
    showError("");
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
    document.getElementById("pair-code").value = "";
    await refresh();
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
