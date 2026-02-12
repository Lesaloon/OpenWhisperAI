import { formatBytes, formatDuration, formatPercent, formatRate } from "./utils/formatters.js";

const modelList = document.querySelector("#modelList");
const hotkeyList = document.querySelector("#hotkeyList");
const latency = document.querySelector("#latency");
const latencyValue = document.querySelector("#latencyValue");
const overlayMode = document.querySelector("#overlayMode");
const overlayPosition = document.querySelector("#overlayPosition");
const overlayStatus = document.querySelector("#overlayStatus");
const overlayHint = document.querySelector("#overlayHint");
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
  modelList.innerHTML = "";

  models.forEach((model) => {
    const card = document.createElement("article");
    card.className = "model-card";

    const progressValue = formatPercent(model.progress);
    const progressLabel = `${formatBytes(model.downloaded)} of ${formatBytes(model.size)}`;
    const etaLabel = model.eta > 0 ? formatDuration(model.eta) : "-";
    const speedLabel = model.speed > 0 ? formatRate(model.speed) : "-";

    card.innerHTML = `
      <header>
        <h3>${model.name}</h3>
        <span>${model.status}</span>
      </header>
      <div class="progress">
        <div class="progress-bar" style="width: ${progressValue}"></div>
      </div>
      <div class="model-meta">
        <div>${progressLabel}</div>
        <div>ETA ${etaLabel}</div>
        <div>${speedLabel}</div>
      </div>
    `;

    modelList.appendChild(card);
  });
}

function renderHotkeys(list) {
  hotkeyList.innerHTML = "";

  list.forEach((hotkey) => {
    const row = document.createElement("div");
    row.className = "hotkey-row";
    row.innerHTML = `
      <div>${hotkey.action}</div>
      <code>${hotkey.keys}</code>
    `;

    hotkeyList.appendChild(row);
  });
}

function updateLatency() {
  latencyValue.textContent = `${latency.value}ms`;
}

function updateOverlayState() {
  overlayStatus.textContent = `Listening · ${document.querySelector("#autoLanguage").checked ? "Auto" : "English"}`;
  overlayHint.textContent = autoPunctuation.checked ? "Auto punctuation on" : "Auto punctuation off";
  overlayHint.classList.toggle("chip", autoPunctuation.checked);

  document.querySelectorAll(".overlay-body .time").forEach((el) => {
    el.style.display = showTimestamps.checked ? "inline" : "none";
  });
}

function updateOverlayMode(value) {
  overlayMode.textContent = value;
  overlayPosition.querySelectorAll("button").forEach((button) => {
    button.classList.toggle("active", button.dataset.value === value);
  });
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
