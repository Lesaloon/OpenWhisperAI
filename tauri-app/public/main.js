const startButton = document.querySelector("#startRecording");
const stopButton = document.querySelector("#stopRecording");
const transcriptOutput = document.querySelector("#transcriptOutput");
const recordingHint = document.querySelector("#recordingHint");
const ipcStatus = document.querySelector("#ipcStatus");
const pttStatus = document.querySelector("#pttStatus");
const hotkeyInput = document.querySelector("#hotkeyInput");
const applyHotkey = document.querySelector("#applyHotkey");
const hotkeyPreview = document.querySelector("#hotkeyPreview");
const outputMode = document.querySelector("#outputMode");

let invokeCommand = null;
let pttState = "idle";
let latestTranscript = "";
let lastTranscriptFetch = "";

const defaultHotkey = {
  key: "space",
  modifiers: { ctrl: true, alt: true, shift: false, meta: false },
};
let pendingHotkey = { ...defaultHotkey };

function setTranscriptOutput(text) {
  if (!transcriptOutput) return;
  transcriptOutput.textContent = text;
}

function setStatus(message) {
  if (recordingHint) {
    recordingHint.textContent = message;
  }
}

function setPttStatus(text) {
  if (pttStatus) {
    pttStatus.textContent = `PTT: ${text}`;
  }
}

function formatHotkey(payload) {
  if (!payload) return "";
  const parts = [];
  if (payload.modifiers.ctrl) parts.push("Ctrl");
  if (payload.modifiers.alt) parts.push("Alt");
  if (payload.modifiers.shift) parts.push("Shift");
  if (payload.modifiers.meta) parts.push("Meta");
  parts.push(payload.key.length === 1 ? payload.key.toUpperCase() : capitalize(payload.key));
  return parts.join(" + ");
}

function capitalize(value) {
  return value.charAt(0).toUpperCase() + value.slice(1);
}

function normalizeKey(eventKey) {
  if (!eventKey) return null;
  if (eventKey === " ") return "space";
  const lower = eventKey.toLowerCase();
  if (lower.length === 1 && lower >= "a" && lower <= "z") {
    return lower;
  }
  if (lower.startsWith("f") && /^f\d{1,2}$/.test(lower)) {
    return lower;
  }
  const map = {
    escape: "escape",
    enter: "enter",
    tab: "tab",
    backspace: "backspace",
    arrowleft: "left",
    arrowright: "right",
    arrowup: "up",
    arrowdown: "down",
    spacebar: "space",
    space: "space",
  };
  return map[lower] ?? null;
}

function isModifierKey(eventKey) {
  return ["Shift", "Control", "Alt", "Meta"].includes(eventKey);
}

function handleHotkeyCapture(event) {
  if (isModifierKey(event.key)) return;
  event.preventDefault();
  const key = normalizeKey(event.key);
  if (!key) {
    setStatus("Unsupported key. Use A-Z, F1-F12, arrows, or space.");
    return;
  }
  pendingHotkey = {
    key,
    modifiers: {
      ctrl: event.ctrlKey,
      alt: event.altKey,
      shift: event.shiftKey,
      meta: event.metaKey,
    },
  };
  if (hotkeyInput) {
    hotkeyInput.value = formatHotkey(pendingHotkey);
  }
  if (hotkeyPreview) {
    hotkeyPreview.textContent = `Pending: ${formatHotkey(pendingHotkey)}`;
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
  const isRecording = pttState === "capturing" || pttState === "processing";
  if (startButton) startButton.disabled = isRecording;
  if (stopButton) stopButton.disabled = !isRecording;
  setPttStatus(pttState);
  if (isRecording) {
    setStatus("Recording... Press hotkey again to stop.");
  } else if (pttState === "error") {
    setStatus(normalized.message || "Error");
  } else {
    setStatus("Press the hotkey to start/stop.");
  }
}

async function refreshLastTranscript() {
  if (!invokeCommand) return;
  try {
    const text = await invokeCommand("ipc_get_last_transcript");
    if (typeof text === "string" && text.length > 0 && text !== lastTranscriptFetch) {
      lastTranscriptFetch = text;
      latestTranscript = text;
      setTranscriptOutput(text);
    }
  } catch (error) {
    setStatus("Transcript sync failed");
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
  if (!invoke) {
    setStatus("IPC unavailable");
    return;
  }

  invokeCommand = invoke;
  refreshLastTranscript();
  setInterval(refreshLastTranscript, 1500);
  setInterval(async () => {
    try {
      const ptt = await invoke("ipc_ptt_get_state");
      applyPttState(ptt);
    } catch (error) {
      setStatus("PTT status unavailable");
    }
  }, 500);

  try {
    const ptt = await invoke("ipc_ptt_get_state");
    applyPttState(ptt);
    const settings = await invoke("ipc_get_settings");
    if (outputMode && settings?.output_mode) {
      outputMode.value = settings.output_mode;
    }
  } catch (error) {
    setStatus("Backend unavailable");
  }

  if (listen) {
    listen("ptt_state", (event) => {
      applyPttState(event?.payload);
    });
    listen("ptt_transcription", (event) => {
      if (typeof event?.payload === "string") {
        latestTranscript = event.payload;
        setTranscriptOutput(event.payload || "(empty transcript)");
      }
    });
    listen("ptt_error", (event) => {
      if (typeof event?.payload === "string") {
        setStatus(event.payload);
      }
    });
  }
}

if (hotkeyInput) {
  hotkeyInput.value = formatHotkey(defaultHotkey);
  hotkeyInput.addEventListener("keydown", handleHotkeyCapture);
}

if (applyHotkey) {
  applyHotkey.addEventListener("click", async () => {
    if (!invokeCommand) {
      setStatus("IPC unavailable");
      return;
    }
    if (!pendingHotkey) {
      setStatus("Press a shortcut first");
      return;
    }
    try {
      const next = await invokeCommand("ipc_ptt_set_hotkey", pendingHotkey);
      pendingHotkey = next;
      if (hotkeyPreview) {
        hotkeyPreview.textContent = `Current: ${formatHotkey(next)}`;
      }
      setStatus("Hotkey updated");
    } catch (error) {
      setStatus("Hotkey update failed");
    }
  });
}

if (outputMode) {
  outputMode.addEventListener("change", async () => {
    if (!invokeCommand) {
      setStatus("IPC unavailable");
      return;
    }
    try {
      await invokeCommand("ipc_update_settings", { update: { output_mode: outputMode.value } });
      setStatus("Output mode updated");
    } catch (error) {
      setStatus("Output mode update failed");
    }
  });
}

if (startButton) {
  startButton.addEventListener("click", async () => {
    if (!invokeCommand) {
      setStatus("IPC unavailable");
      return;
    }
    if (pttState === "capturing" || pttState === "processing") {
      return;
    }
    startButton.disabled = true;
    try {
      const next = await invokeCommand("ipc_ptt_toggle_recording");
      applyPttState(next);
    } catch (error) {
      setStatus("Failed to start recording");
      startButton.disabled = false;
    }
  });
}

if (stopButton) {
  stopButton.addEventListener("click", async () => {
    if (!invokeCommand) {
      setStatus("IPC unavailable");
      return;
    }
    if (pttState !== "capturing" && pttState !== "processing") {
      return;
    }
    stopButton.disabled = true;
    try {
      const next = await invokeCommand("ipc_ptt_toggle_recording");
      applyPttState(next);
    } catch (error) {
      setStatus("Failed to stop recording");
      stopButton.disabled = false;
    }
  });
}

setTranscriptOutput("Latest transcript will appear here.");
setPttStatus("idle");
initializeBackendBridge();
