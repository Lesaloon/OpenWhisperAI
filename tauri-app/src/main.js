import { formatBytes, formatDuration, formatPercent, formatRate } from "./utils/formatters.js";

const modelList = document.querySelector("#modelList");
const hotkeyList = document.querySelector("#hotkeyList");
const latency = document.querySelector("#latency");
const latencyValue = document.querySelector("#latencyValue");
const overlayMode = document.querySelector("#overlayMode");
const overlayPosition = document.querySelector("#overlayPosition");
const overlayStatus = document.querySelector("#overlayStatus");
const overlayHint = document.querySelector("#overlayHint");
const backendLog = document.querySelector("#backendLog");
const showTimestamps = document.querySelector("#showTimestamps");
const autoPunctuation = document.querySelector("#autoPunctuation");
const hotkeySearch = document.querySelector("#hotkeySearch");

const models = [
  {
    name: "Whisper Large v3",
    status: "Downloading",
    progress: 68,
    size: 3120000000,
    downloaded: 2120000000,
    eta: 180,
    speed: 45000000,
  },
  {
    name: "Whisper Medium",
    status: "Queued",
    progress: 0,
    size: 1450000000,
    downloaded: 0,
    eta: 0,
    speed: 0,
  },
  {
    name: "Whisper Small",
    status: "Ready",
    progress: 100,
    size: 466000000,
    downloaded: 466000000,
    eta: 0,
    speed: 0,
  },
];

const hotkeys = [
  { action: "Start / Stop recording", keys: "Ctrl + Shift + R" },
  { action: "Toggle overlay", keys: "Ctrl + Shift + O" },
  { action: "Push last segment", keys: "Ctrl + Shift + Enter" },
  { action: "Mark highlight", keys: "Ctrl + Shift + H" },
  { action: "Open settings", keys: "Ctrl + ," },
  { action: "Export transcript", keys: "Ctrl + Shift + E" },
  { action: "Mute input", keys: "Ctrl + Shift + M" },
];

function renderModels() {
  modelList.replaceChildren();
  const fragment = document.createDocumentFragment();

  models.forEach((model) => {
    const card = document.createElement("article");
    card.className = "model-card";

    const progressValue = formatPercent(model.progress);
    const progressLabel = `${formatBytes(model.downloaded)} of ${formatBytes(model.size)}`;
    const etaLabel = model.eta > 0 ? formatDuration(model.eta) : "-";
    const speedLabel = model.speed > 0 ? formatRate(model.speed) : "-";

    const header = document.createElement("header");
    const title = document.createElement("h3");
    title.textContent = model.name;
    const status = document.createElement("span");
    status.textContent = model.status;
    header.append(title, status);

    const progress = document.createElement("div");
    progress.className = "progress";
    const progressBar = document.createElement("div");
    progressBar.className = "progress-bar";
    progressBar.style.width = progressValue;
    progress.append(progressBar);

    const meta = document.createElement("div");
    meta.className = "model-meta";
    const progressMeta = document.createElement("div");
    progressMeta.textContent = progressLabel;
    const etaMeta = document.createElement("div");
    etaMeta.textContent = `ETA ${etaLabel}`;
    const speedMeta = document.createElement("div");
    speedMeta.textContent = speedLabel;
    meta.append(progressMeta, etaMeta, speedMeta);

    card.append(header, progress, meta);
    fragment.appendChild(card);
  });

  modelList.appendChild(fragment);
}

function renderHotkeys(list) {
  hotkeyList.replaceChildren();
  const fragment = document.createDocumentFragment();

  list.forEach((hotkey) => {
    const row = document.createElement("div");
    row.className = "hotkey-row";

    const action = document.createElement("div");
    action.textContent = hotkey.action;
    const keys = document.createElement("code");
    keys.textContent = hotkey.keys;

    row.append(action, keys);
    fragment.appendChild(row);
  });

  hotkeyList.appendChild(fragment);
}

function updateLatency() {
  latencyValue.textContent = `${latency.value}ms`;
}

const backendStateLabels = {
  idle: "Idle",
  recording: "Listening",
  processing: "Processing",
  error: "Error",
};

let backendState = "idle";

function setBackendLog(text) {
  if (backendLog) {
    backendLog.textContent = text;
  }
}

function updateOverlayState() {
  const languageLabel = document.querySelector("#autoLanguage").checked ? "Auto" : "English";
  const stateLabel = backendStateLabels[backendState] || "Idle";
  overlayStatus.textContent = `${stateLabel} · ${languageLabel}`;
  overlayHint.textContent = autoPunctuation.checked ? "Auto punctuation on" : "Auto punctuation off";
  overlayHint.classList.toggle("chip", autoPunctuation.checked);

  document.querySelectorAll(".overlay-body .time").forEach((el) => {
    el.style.display = showTimestamps.checked ? "inline" : "none";
  });
}

function updateOverlayMode(value) {
  overlayMode.textContent = value;
  overlayPosition.querySelectorAll("button").forEach((button) => {
    const isActive = button.dataset.value === value;
    button.classList.toggle("active", isActive);
    button.setAttribute("aria-pressed", isActive.toString());
  });
}

function normalizeBackendState(state) {
  if (!state) return { status: "idle" };
  if (typeof state === "string") return { status: state };
  if (typeof state === "object") {
    const [key] = Object.keys(state);
    if (!key) return { status: "idle" };
    return { status: key, message: state[key]?.message };
  }
  return { status: "idle" };
}

function applyBackendState(state) {
  const normalized = normalizeBackendState(state);
  backendState = normalized.status;
  updateOverlayState();
  if (normalized.status === "error" && normalized.message) {
    setBackendLog(normalized.message);
  }
}

async function initializeBackendBridge() {
  const tauri = window.__TAURI__;
  const invoke = tauri?.core?.invoke ?? tauri?.invoke;
  const listen = tauri?.event?.listen;

  if (invoke) {
    try {
      const state = await invoke("ipc_get_state");
      applyBackendState(state);
      const logs = await invoke("ipc_get_logs");
      if (Array.isArray(logs) && logs.length > 0) {
        setBackendLog(logs[logs.length - 1].message);
      }
    } catch (error) {
      setBackendLog("Backend unavailable");
    }
  }

  if (listen) {
    listen("backend-log", (event) => {
      if (event?.payload?.message) {
        setBackendLog(event.payload.message);
      }
    });
  }
}

function handleHotkeySearch(event) {
  const query = event.target.value.toLowerCase();
  const filtered = hotkeys.filter((hotkey) => hotkey.action.toLowerCase().includes(query));
  renderHotkeys(filtered);
}

renderModels();
renderHotkeys(hotkeys);
updateLatency();
updateOverlayState();
initializeBackendBridge();

latency.addEventListener("input", updateLatency);
showTimestamps.addEventListener("change", updateOverlayState);
autoPunctuation.addEventListener("change", updateOverlayState);
document.querySelector("#autoLanguage").addEventListener("change", updateOverlayState);
hotkeySearch.addEventListener("input", handleHotkeySearch);

overlayPosition.addEventListener("click", (event) => {
  const button = event.target.closest("button");
  if (!button) return;
  updateOverlayMode(button.dataset.value);
});
