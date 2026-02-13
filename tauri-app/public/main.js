import { formatBytes, formatDuration, formatPercent, formatRate } from "./utils/formatters.js";

const modelList = document.querySelector("#modelList");
const hotkeyList = document.querySelector("#hotkeyList");
const latency = document.querySelector("#latency");
const latencyValue = document.querySelector("#latencyValue");
const overlayMode = document.querySelector("#overlayMode");
const overlayPosition = document.querySelector("#overlayPosition");
const overlayStatus = document.querySelector("#overlayStatus");
const overlayBody = document.querySelector("#overlayBody");
const overlayHint = document.querySelector("#overlayHint");
const transcriptOutput = document.querySelector("#transcriptOutput");
const backendLog = document.querySelector("#backendLog");
const pttManual = document.querySelector("#pttManual");
const helloButton = document.querySelector("#helloButton");
const ipcStatus = document.querySelector("#ipcStatus");
const ipcDetails = document.querySelector("#ipcDetails");
const showTimestamps = document.querySelector("#showTimestamps");
const autoPunctuation = document.querySelector("#autoPunctuation");
const hotkeySearch = document.querySelector("#hotkeySearch");
const activeModelValue = document.querySelector("#activeModelValue");
const queueValue = document.querySelector("#queueValue");

const models = [];
let invokeCommand = null;

const hotkeys = [
  { action: "Push-to-talk", keys: "Ctrl + Alt + Space" },
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

  if (models.length === 0) {
    const empty = document.createElement("p");
    empty.className = "muted";
    empty.textContent = "No model status yet. Connect to the backend.";
    modelList.appendChild(empty);
    return;
  }

  models.forEach((model) => {
    const card = document.createElement("article");
    card.className = "model-card";

    const progressValue = formatPercent(model.progress);
    const progressLabel = model.size > 0
      ? `${formatBytes(model.downloaded)} of ${formatBytes(model.size)}`
      : "Size unknown";
    const etaLabel = model.eta > 0 ? formatDuration(model.eta) : "-";
    const speedLabel = model.speed > 0 ? formatRate(model.speed) : "-";
    const statusLabel = formatModelStatus(model.status);

    const header = document.createElement("header");
    const title = document.createElement("h3");
    title.textContent = model.name;
    const status = document.createElement("span");
    status.textContent = statusLabel;
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

    const actions = document.createElement("div");
    actions.className = "model-actions";
    const actionButton = document.createElement("button");
    actionButton.type = "button";
    actionButton.className = "model-action";
    const action = resolveModelAction(model);
    actionButton.textContent = action.label;
    actionButton.disabled = action.disabled;
    actionButton.dataset.action = action.action;
    actionButton.dataset.modelId = model.id;
    if (action.variant) {
      actionButton.classList.add(action.variant);
    }
    if (!action.disabled && action.action !== "none") {
      actionButton.addEventListener("click", () => {
        handleModelAction(model.id, action.action);
      });
    }
    actions.append(actionButton);

    card.append(header, progress, meta, actions);
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
  armed: "Ready",
  capturing: "Listening",
  recording: "Listening",
  processing: "Processing",
  error: "Error",
};

let backendState = "idle";
let pttState = "idle";
let latestTranscript = "";
let lastTranscriptFetch = "";

function setBackendLog(text) {
  if (backendLog) {
    backendLog.textContent = text;
  }
  if (typeof text === "string") {
    console.log("[OpenWhisperAI]", text);
  }
}

function updateOverlayState() {
  const languageLabel = document.querySelector("#autoLanguage").checked ? "Auto" : "English";
  const stateLabel = backendStateLabels[pttState] || backendStateLabels[backendState] || "Idle";
  overlayStatus.textContent = `${stateLabel} · ${languageLabel}`;
  overlayHint.textContent = autoPunctuation.checked ? "Auto punctuation on" : "Auto punctuation off";
  overlayHint.classList.toggle("chip", autoPunctuation.checked);

  document.querySelectorAll(".overlay-body .time").forEach((el) => {
    el.style.display = showTimestamps.checked ? "inline" : "none";
  });
}

function setOverlayBody(text, muted = false) {
  if (!overlayBody) return;
  overlayBody.replaceChildren();
  const line = document.createElement("p");
  if (muted) line.classList.add("muted");
  line.textContent = text;
  overlayBody.appendChild(line);
}

function setTranscriptOutput(text) {
  if (!transcriptOutput) return;
  transcriptOutput.textContent = text;
}

async function refreshLastTranscript() {
  if (!invokeCommand) return;
  try {
    const text = await invokeCommand("ipc_get_last_transcript");
    if (typeof text === "string" && text.length > 0 && text !== lastTranscriptFetch) {
      lastTranscriptFetch = text;
      latestTranscript = text;
      setOverlayBody(text);
      setTranscriptOutput(text);
      setBackendLog("transcription synced");
    }
  } catch (error) {
    setBackendLog("transcription sync failed");
  }
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

function formatModelStatus(status) {
  const normalized = String(status || "unknown").replace(/_/g, " ");
  return normalized.charAt(0).toUpperCase() + normalized.slice(1);
}

function resolveModelAction(model) {
  const status = String(model.status || "unknown").toLowerCase();
  if (model.active) {
    return { label: "Selected", action: "none", disabled: true, variant: "selected" };
  }
  if (status === "downloading" || status === "queued") {
    return { label: "Downloading", action: "none", disabled: true, variant: "busy" };
  }
  if (status === "ready" || status === "installed") {
    return { label: "Select", action: "select", disabled: false, variant: "primary" };
  }
  if (status === "failed") {
    return { label: "Retry Download", action: "download", disabled: false, variant: "warn" };
  }
  return { label: "Download", action: "download", disabled: false, variant: "primary" };
}

function applyModelPayload(payload) {
  if (!payload?.models) return;
  models.splice(
    0,
    models.length,
    ...payload.models.map((model) => ({
      id: model.id ?? model.name,
      name: model.name,
      status: String(model.status ?? "unknown"),
      progress: Math.round((model.progress ?? 0) * 100) / 100,
      size: model.total_bytes ?? 0,
      downloaded: model.downloaded_bytes ?? 0,
      eta: model.eta_seconds ?? 0,
      speed: model.speed_bytes_per_sec ?? 0,
      active: Boolean(model.active),
    }))
  );
  renderModels();
  updateModelStats(payload);
}

function updateModelStats(payload) {
  if (activeModelValue) {
    activeModelValue.textContent = payload?.active_model || "None";
  }
  if (queueValue) {
    const count = payload?.queue_count ?? 0;
    queueValue.textContent = `${count} download${count === 1 ? "" : "s"}`;
  }
}

async function handleModelAction(modelId, action) {
  if (!invokeCommand) {
    setBackendLog("model action failed: IPC unavailable");
    return;
  }
  try {
    if (action === "download") {
      await invokeCommand("ipc_model_download", { model: modelId });
    } else if (action === "select") {
      await invokeCommand("ipc_model_select", { model: modelId });
    }
  } catch (error) {
    setBackendLog(`model action failed: ${error}`);
  }
}

function applyBackendState(state) {
  const normalized = normalizeBackendState(state);
  backendState = normalized.status;
  updateOverlayState();
  if (normalized.status === "error" && normalized.message) {
    setBackendLog(normalized.message);
  }
}

function normalizePttState(state) {
  if (!state) return { status: "idle" };
  if (typeof state === "string") return { status: state };
  if (typeof state === "object") {
    const [key] = Object.keys(state);
    if (!key) return { status: "idle" };
    return { status: key, message: state[key]?.message };
  }
  return { status: "idle" };
}

function applyPttState(state) {
  const normalized = normalizePttState(state);
  pttState = normalized.status;
  updateOverlayState();
  if (pttManual) {
    const isRecording = pttState === "capturing" || pttState === "processing";
    pttManual.textContent = isRecording ? "Stop Recording" : "Start Recording";
    pttManual.setAttribute("aria-pressed", isRecording.toString());
  }
  if (normalized.status === "error" && normalized.message) {
    setOverlayBody(normalized.message, true);
  }
}

async function initializeBackendBridge() {
  const tauri = window.__TAURI__;
  let invoke = tauri?.core?.invoke ?? tauri?.invoke;
  const listen = tauri?.event?.listen;

  if (!invoke && typeof window.__TAURI_IPC__ === "function") {
    const ipc = window.__TAURI_IPC__;
    const transformCallback = (callback, once = false) => {
      const id = window.crypto?.getRandomValues
        ? window.crypto.getRandomValues(new Uint32Array(1))[0]
        : Math.floor(Math.random() * 1_000_000);
      Object.defineProperty(window, `_${id}`, {
        value: (response) => {
          if (once) {
            Reflect.deleteProperty(window, `_${id}`);
          }
          callback?.(response);
        },
        writable: false,
        configurable: true,
      });
      return id;
    };

    invoke = (command, payload = {}) =>
      new Promise((resolve, reject) => {
        const callback = transformCallback((value) => resolve(value), true);
        const error = transformCallback((value) => reject(value), true);
        ipc({ cmd: command, callback, error, ...payload });
      });
  }

  if (ipcStatus) {
    ipcStatus.textContent = invoke ? "IPC: ready" : "IPC: missing";
  }
  if (ipcDetails) {
    const keys = tauri ? Object.keys(tauri).join(",") : "-";
    const hasIpc = typeof window.__TAURI_IPC__ === "function";
    ipcDetails.textContent = `IPC details: tauri=${Boolean(tauri)}, invoke=${Boolean(invoke)}, ipc=${hasIpc}, keys=${keys}`;
  }

  if (!invoke) {
    setOverlayBody("Tauri bridge unavailable", true);
    if (tauri) {
      setBackendLog("__TAURI__ present but invoke missing");
    } else {
      setBackendLog("__TAURI__ bridge not detected");
    }
    return;
  }

  invokeCommand = invoke;
  refreshLastTranscript();
  setInterval(refreshLastTranscript, 1500);

  if (invoke) {
    try {
      const state = await invoke("ipc_get_state");
      applyBackendState(state);
      const ptt = await invoke("ipc_ptt_get_state");
      applyPttState(ptt);
      const modelPayload = await invoke("ipc_get_models");
      applyModelPayload(modelPayload);
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
    listen("ptt_state", (event) => {
      applyPttState(event?.payload);
      if (pttState === "capturing") {
        setOverlayBody("Listening…");
      } else if (pttState === "processing") {
        setOverlayBody("Transcribing…", true);
      } else if (pttState === "armed") {
        setOverlayBody(latestTranscript || "Ready. Hold Ctrl + Alt + Space to dictate.", true);
        if (!latestTranscript && invokeCommand) {
          invokeCommand("ipc_get_last_transcript")
            .then((text) => {
              if (typeof text === "string" && text.length > 0) {
                latestTranscript = text;
                setOverlayBody(text);
                setTranscriptOutput(text);
              }
            })
            .catch(() => {});
        }
      }
    });
    listen("ptt_transcription", (event) => {
      if (typeof event?.payload === "string") {
        latestTranscript = event.payload;
        setOverlayBody(event.payload || "(empty transcript)");
        setTranscriptOutput(event.payload || "(empty transcript)");
        setBackendLog("transcription received");
      }
    });
    listen("ptt_error", (event) => {
      if (typeof event?.payload === "string") {
        setOverlayBody(event.payload, true);
      }
    });
    listen("model-download-status", (event) => {
      applyModelPayload(event?.payload);
    });
  }

  if (!listen) {
    const poll = async () => {
      try {
        const ptt = await invoke("ipc_ptt_get_state");
        applyPttState(ptt);
        const modelPayload = await invoke("ipc_get_models");
        applyModelPayload(modelPayload);
      } catch (error) {
        setBackendLog("Polling failed");
      }
    };
    setInterval(poll, 1000);
  }

  if (pttManual) {
    pttManual.addEventListener("click", async () => {
      try {
        setBackendLog("ptt toggle clicked");
        if (!invoke) {
          setOverlayBody("PTT unavailable (no IPC)", true);
          return;
        }
        const next = await invoke("ipc_ptt_toggle_recording");
        applyPttState(next);
      } catch (error) {
        setBackendLog(`ptt toggle failed: ${error}`);
        setOverlayBody("Failed to toggle PTT", true);
      }
    });
  }

  if (helloButton) {
    helloButton.addEventListener("click", async () => {
      try {
        setBackendLog("hello clicked");
        setOverlayBody("Hello clicked", true);
        if (!invoke) {
          setOverlayBody("Hello (no IPC)", true);
          return;
        }
        const reply = await invoke("ipc_hello");
        if (reply) {
          setOverlayBody(String(reply), true);
        }
      } catch (error) {
        setBackendLog(`hello failed: ${error}`);
        setOverlayBody("Hello failed", true);
      }
    });
  }

  if (!helloButton) {
    setBackendLog("hello button not found in DOM");
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
setOverlayBody("Ready. Hold Ctrl + Alt + Space to dictate.", true);
setTranscriptOutput("Latest transcript will appear here.");
if (helloButton) {
  helloButton.textContent = "Hello (JS)";
  helloButton.dataset.js = "loaded";
  helloButton.addEventListener("click", () => {
    setOverlayBody("Hello JS handler fired", true);
  });
}
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
