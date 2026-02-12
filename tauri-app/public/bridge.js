(() => {
  const domReady = (callback) => {
    if (document.readyState === "loading") {
      document.addEventListener("DOMContentLoaded", callback, { once: true });
    } else {
      callback();
    }
  };

  const stateLabels = {
    idle: "Idle",
    recording: "Listening",
    processing: "Processing",
    error: "Error",
  };

  const select = (selector) => document.querySelector(selector);

  const overlayStatus = select("#overlayStatus");
  const overlaySummary = select("#overlaySummary");
  const backendLog = select("#backendLog");
  const autoLanguage = select("#autoLanguage");
  const activeModelName = select("#activeModelName");
  const downloadQueueCount = select("#downloadQueueCount");
  const modelList = select("#modelList");

  let latestState = { status: "idle" };
  let latestModels = [];

  const formatBytes = (bytes) => {
    if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
    const units = ["B", "KB", "MB", "GB", "TB"];
    const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
    const value = bytes / Math.pow(1024, index);
    return `${value.toFixed(value >= 100 || index === 0 ? 0 : 1)} ${units[index]}`;
  };

  const formatPercent = (value) => {
    if (!Number.isFinite(value)) return "0%";
    return `${Math.max(0, Math.min(100, value)).toFixed(0)}%`;
  };

  const formatRate = (bytesPerSecond) => {
    if (!Number.isFinite(bytesPerSecond) || bytesPerSecond <= 0) return "-";
    return `${formatBytes(bytesPerSecond)}/s`;
  };

  const formatDuration = (seconds) => {
    if (!Number.isFinite(seconds) || seconds <= 0) return "-";
    const mins = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    if (mins <= 0) return `${secs}s`;
    return `${mins}m ${secs}s`;
  };

  const setBackendLog = (text) => {
    if (backendLog && typeof text === "string") {
      backendLog.textContent = text;
    }
  };

  const normalizeBackendState = (state) => {
    if (!state) return { status: "idle" };
    if (typeof state === "string") return { status: state };
    if (typeof state === "object") {
      if (state.status) {
        return { status: String(state.status), message: state.message };
      }
      const [key] = Object.keys(state);
      if (key) {
        const value = state[key];
        if (value && typeof value === "object" && "message" in value) {
          return { status: key, message: value.message };
        }
        return { status: key };
      }
    }
    return { status: "idle" };
  };

  const updateOverlayState = (state) => {
    const normalized = normalizeBackendState(state);
    latestState = normalized;
    const label = stateLabels[normalized.status] || "Idle";
    const languageLabel = autoLanguage?.checked ? "Auto" : "English";
    if (overlayStatus) {
      overlayStatus.textContent = `${label} - ${languageLabel}`;
    }
    if (overlaySummary) {
      overlaySummary.textContent = label === "Idle" ? "Standby" : `${label} overlay`;
    }
    if (normalized.status === "error" && normalized.message) {
      setBackendLog(normalized.message);
    }
  };

  const numberFrom = (value) => (Number.isFinite(value) ? value : Number(value) || 0);

  const normalizeModel = (model) => {
    const totalBytes = numberFrom(model.total_bytes ?? model.size ?? model.totalBytes);
    const downloaded = numberFrom(model.downloaded_bytes ?? model.downloaded ?? model.downloadedBytes);
    const speed = numberFrom(model.speed_bytes_per_sec ?? model.speed ?? model.rate);
    const eta = numberFrom(model.eta_seconds ?? model.eta);
    const progress =
      Number.isFinite(model.progress) && model.progress >= 0
        ? model.progress
        : totalBytes > 0
          ? (downloaded / totalBytes) * 100
          : 0;
    const status = String(model.status ?? model.state ?? model.phase ?? "unknown").toLowerCase();

    return {
      id: model.id ?? model.name ?? "model",
      name: model.name ?? "Model",
      status,
      totalBytes,
      downloaded,
      speed,
      eta,
      progress,
      active: Boolean(model.active ?? model.is_active ?? model.isActive),
    };
  };

  const statusInfo = (status) => {
    switch (status) {
      case "ready":
      case "installed":
        return { label: "Installed", className: "model-status--installed" };
      case "downloading":
        return { label: "Downloading", className: "" };
      case "queued":
      case "pending":
        return { label: "Queued", className: "" };
      case "failed":
      case "error":
        return { label: "Failed", className: "" };
      default:
        return { label: "Unknown", className: "" };
    }
  };

  const renderModels = (models) => {
    if (!modelList) return;
    modelList.replaceChildren();

    if (!models || models.length === 0) {
      const empty = document.createElement("div");
      empty.className = "model-empty";
      empty.textContent = "No models reported yet.";
      modelList.appendChild(empty);
      return;
    }

    const fragment = document.createDocumentFragment();

    models.forEach((model) => {
      const card = document.createElement("article");
      card.className = "model-card";

      const header = document.createElement("header");
      header.className = "model-header";

      const title = document.createElement("h3");
      title.className = "model-title";
      title.textContent = model.name;

      const status = document.createElement("span");
      const statusMeta = statusInfo(model.status);
      status.className = `model-status ${statusMeta.className}`.trim();
      status.textContent = statusMeta.label;

      header.append(title, status);

      const progress = document.createElement("div");
      progress.className = "model-progress";
      const progressBar = document.createElement("div");
      progressBar.className = "model-progress-bar";
      progressBar.style.width = formatPercent(model.progress);
      progress.appendChild(progressBar);

      const meta = document.createElement("div");
      meta.className = "model-meta";
      const progressMeta = document.createElement("span");
      progressMeta.textContent = `${formatBytes(model.downloaded)} of ${formatBytes(model.totalBytes)}`;
      const etaMeta = document.createElement("span");
      etaMeta.textContent = `ETA ${formatDuration(model.eta)}`;
      const speedMeta = document.createElement("span");
      speedMeta.textContent = formatRate(model.speed);
      meta.append(progressMeta, etaMeta, speedMeta);

      card.append(header, progress, meta);
      fragment.appendChild(card);
    });

    modelList.appendChild(fragment);
  };

  const updateHeroStats = (models, payload) => {
    const activeName = payload?.active_model ?? payload?.activeModel;
    const activeModel =
      models.find((model) => model.active) ||
      models.find((model) => model.status === "ready" || model.status === "installed") ||
      models[0];
    if (activeModelName) {
      activeModelName.textContent = activeName ?? activeModel?.name ?? "Unknown";
    }

    const queueCount =
      typeof payload?.queue_count === "number"
        ? payload.queue_count
        : models.filter((model) => ["downloading", "queued", "pending"].includes(model.status)).length;
    if (downloadQueueCount) {
      downloadQueueCount.textContent = `${queueCount} download${queueCount === 1 ? "" : "s"}`;
    }
  };

  const handleModelUpdate = (payload) => {
    const models = Array.isArray(payload)
      ? payload
      : Array.isArray(payload?.models)
        ? payload.models
        : null;
    if (!models) return;
    latestModels = models.map(normalizeModel);
    renderModels(latestModels);
    updateHeroStats(latestModels, payload);
  };

  const installBridgeListeners = (listen) => {
    if (!listen) return;
    listen("backend-state", (event) => {
      updateOverlayState(event?.payload ?? event);
    });
    listen("backend-log", (event) => {
      if (event?.payload?.message) {
        setBackendLog(event.payload.message);
      }
    });
    listen("model-download-status", (event) => {
      handleModelUpdate(event?.payload ?? event);
    });
  };

  const buildMockBridge = () => {
    const listeners = new Map();
    const logs = [];
    let currentState = { status: "idle" };
    const models = [
      {
        id: "whisper-large-v3",
        name: "Whisper Large v3",
        status: "downloading",
        totalBytes: 3120000000,
        downloaded: 1580000000,
        speed: 52000000,
        eta: 180,
        progress: 50,
        active: true,
      },
      {
        id: "whisper-medium",
        name: "Whisper Medium",
        status: "queued",
        totalBytes: 1450000000,
        downloaded: 0,
        speed: 0,
        eta: 0,
        progress: 0,
      },
      {
        id: "whisper-small",
        name: "Whisper Small",
        status: "ready",
        totalBytes: 466000000,
        downloaded: 466000000,
        speed: 0,
        eta: 0,
        progress: 100,
      },
    ];

    const emit = (event, payload) => {
      const handlers = listeners.get(event);
      if (!handlers) return;
      handlers.forEach((handler) => {
        try {
          handler({ event, payload });
        } catch (error) {
          setBackendLog("Bridge handler error");
        }
      });
    };

    const listen = (event, handler) => {
      if (!listeners.has(event)) {
        listeners.set(event, new Set());
      }
      listeners.get(event).add(handler);
      return Promise.resolve(() => listeners.get(event)?.delete(handler));
    };

    const pushLog = (message) => {
      if (!message) return;
      logs.push({ message, timestamp: Date.now() });
      emit("backend-log", { message });
    };

    const normalizeEventName = (event) => {
      if (!event) return "";
      if (typeof event === "string") return event;
      if (typeof event === "object") {
        if (event.type) return event.type;
        const [key] = Object.keys(event);
        if (key) return key;
      }
      return "";
    };

    const applyEvent = (event) => {
      const name = normalizeEventName(event).toLowerCase();
      switch (name) {
        case "start_recording":
        case "startrecording":
          currentState = { status: "recording" };
          pushLog("Recording started");
          break;
        case "stop_recording":
        case "stoprecording":
        case "start_processing":
        case "startprocessing":
          currentState = { status: "processing" };
          pushLog("Processing audio");
          break;
        case "finish_processing":
        case "finishprocessing":
          currentState = { status: "idle" };
          pushLog("Processing complete");
          break;
        case "fail":
          currentState = { status: "error", message: event?.message ?? "Backend error" };
          pushLog(currentState.message);
          break;
        case "reset":
          currentState = { status: "idle" };
          pushLog("Reset complete");
          break;
        default:
          return currentState;
      }
      emit("backend-state", currentState);
      return currentState;
    };

    const invoke = (command, payload) => {
      switch (command) {
        case "ipc_get_state":
          return Promise.resolve(currentState);
        case "ipc_get_logs":
          return Promise.resolve([...logs]);
        case "ipc_send_event":
          return Promise.resolve(applyEvent(payload));
        case "ipc_get_models":
          return Promise.resolve({ models });
        default:
          return Promise.reject(new Error(`Unknown command: ${command}`));
      }
    };

    const tickModels = () => {
      const activeDownload = models.find((model) => model.status === "downloading");
      if (activeDownload) {
        const remaining = activeDownload.totalBytes - activeDownload.downloaded;
        if (remaining > 0) {
          const increment = Math.min(activeDownload.speed, remaining);
          activeDownload.downloaded += increment;
          activeDownload.progress = (activeDownload.downloaded / activeDownload.totalBytes) * 100;
          const etaSeconds = activeDownload.speed > 0 ? Math.ceil(remaining / activeDownload.speed) : 0;
          activeDownload.eta = etaSeconds;
        }
        if (activeDownload.downloaded >= activeDownload.totalBytes) {
          activeDownload.status = "ready";
          activeDownload.speed = 0;
          activeDownload.eta = 0;
          activeDownload.progress = 100;
          pushLog(`${activeDownload.name} installed`);
          const next = models.find((model) => model.status === "queued");
          if (next) {
            next.status = "downloading";
            next.speed = 38000000;
            next.eta = Math.ceil(next.totalBytes / next.speed);
          }
        }
      }
      emit("model-download-status", { models });
    };

    const tickState = () => {
      const cycle = ["idle", "recording", "processing"];
      const currentIndex = cycle.indexOf(currentState.status);
      const next = cycle[(currentIndex + 1) % cycle.length];
      currentState = { status: next };
      emit("backend-state", currentState);
    };

    setInterval(tickModels, 1000);
    setInterval(tickState, 12000);
    emit("model-download-status", { models });

    return {
      core: { invoke },
      event: { listen },
      __mock: true,
    };
  };

  const setupBridge = (tauri) => {
    const invoke = tauri?.core?.invoke ?? tauri?.invoke;
    const listen = tauri?.event?.listen;

    installBridgeListeners(listen);

    if (invoke) {
      invoke("ipc_get_state")
        .then(updateOverlayState)
        .catch(() => {
          updateOverlayState(latestState);
        });
      invoke("ipc_get_logs")
        .then((entries) => {
          if (Array.isArray(entries) && entries.length > 0) {
            setBackendLog(entries[entries.length - 1].message);
          }
        })
        .catch(() => {
          setBackendLog("Backend unavailable");
        });
      invoke("ipc_get_models")
        .then(handleModelUpdate)
        .catch(() => {
          if (latestModels.length === 0) {
            renderModels([]);
          }
        });
    }
  };

  domReady(() => {
    const existing = window.__TAURI__;
    if (existing) {
      setupBridge(existing);
      return;
    }

    const mockBridge = buildMockBridge();
    window.__TAURI__ = mockBridge;
    setupBridge(mockBridge);
  });
})();
