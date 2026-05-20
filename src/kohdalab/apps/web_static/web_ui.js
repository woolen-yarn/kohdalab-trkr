const modes = ["signal_monitor", "trkr", "srkr", "strkr", "srkr_2d"];
const titles = {
  signal_monitor: "Signal Monitor",
  trkr: "TRKR",
  srkr: "SRKR",
  strkr: "STRKR",
  srkr_2d: "SRKR 2D",
};

let state = null;
let currentMode = "signal_monitor";
let hydratedForPath = null;
let outputsByMode = {};
let editingConfig = false;

const $ = (id) => document.getElementById(id);

async function api(path, body = null) {
  const options = body
    ? { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }
    : {};
  const response = await fetch(path, options);
  const data = await response.json();
  if (!response.ok || data.error) {
    if (data.state) {
      applyState(data.state);
    }
    throw new Error(data.error || response.statusText);
  }
  applyState(data);
  return data;
}

async function refresh() {
  try {
    const response = await fetch("/api/state");
    applyState(await response.json());
  } catch (error) {
    appendLocalLog(`refresh error: ${error.message}`);
  }
}

function applyState(nextState) {
  state = nextState;
  const profile = state.defaults?.profile || {};
  const configLabel = state.has_config ? `${state.config_path} (${state.config_source})` : "no config loaded";
  $("profile-line").textContent = `${profile.name || "default"} | ${configLabel}`;
  $("job-pill").textContent = state.job?.running ? state.job.status : state.job?.status || "idle";
  $("operation-pill").textContent = state.active_operation || "no operation";
  $("operation-pill").classList.toggle("muted", !state.active_operation);
  $("config-path").value = state.config_path || "";
  if (!editingConfig) {
    $("config-json").value = JSON.stringify(state.config || {}, null, 2);
  }
  if (hydratedForPath !== state.config_path) {
    hydrateForms(state.defaults);
    hydratedForPath = state.config_path;
  }
  renderDevices();
  renderAxisPanels();
  renderRunStatus();
  renderRows();
  renderSnapshot();
  renderLogs();
  drawPlot();
}

function hydrateForms(defaults) {
  const m = defaults.measurements || {};
  setValue("signal-interval", m.signal_monitor?.interval_s ?? 1);
  setValue("signal-points", m.signal_monitor?.n_points ?? 360);

  setRange("trkr", m.trkr?.scan || {});
  setValue("trkr-wait", m.trkr?.wait_s ?? 2);
  $("trkr-return").checked = Boolean(m.trkr?.return_to_zero ?? true);

  setValue("srkr-axis", m.srkr?.scan?.axis || "x");
  setRange("srkr", m.srkr?.scan || {});
  setValue("srkr-wait", m.srkr?.wait_s ?? 2);
  $("srkr-return").checked = Boolean(m.srkr?.return_to_zero ?? true);

  setValue("strkr-fast", m.strkr?.scan?.fast_axis || "t");
  setValue("strkr-slow", m.strkr?.scan?.slow_axis || "x");
  ["t", "x", "y"].forEach((axis) => setRange(`strkr-${axis}`, m.strkr?.scan?.ranges?.[axis] || {}));
  setValue("strkr-wait", m.strkr?.wait_s ?? 2);
  $("strkr-return-fast").checked = Boolean(m.strkr?.return_to_zero?.fast_axis ?? true);
  $("strkr-return-slow").checked = Boolean(m.strkr?.return_to_zero?.slow_axis ?? true);

  setValue("srkr2d-fast", m.srkr_2d?.scan?.fast_axis || "x");
  setValue("srkr2d-slow", m.srkr_2d?.scan?.slow_axis || "y");
  ["x", "y"].forEach((axis) => setRange(`srkr2d-${axis}`, m.srkr_2d?.scan?.ranges?.[axis] || {}));
  setValue("srkr2d-wait", m.srkr_2d?.wait_s ?? 2);
  $("srkr2d-return-fast").checked = Boolean(m.srkr_2d?.return_to_zero?.fast_axis ?? true);
  $("srkr2d-return-slow").checked = Boolean(m.srkr_2d?.return_to_zero?.slow_axis ?? true);

  outputsByMode = {};
  modes.forEach((mode) => {
    outputsByMode[mode] = { ...(m[mode]?.output || {}) };
  });
  applyOutput(currentMode);
}

function setValue(id, value) {
  const el = $(id);
  if (el) el.value = value;
}

function setRange(prefix, range) {
  setValue(`${prefix}-min`, range.min ?? 0);
  setValue(`${prefix}-max`, range.max ?? 0);
  setValue(`${prefix}-step`, range.step ?? 1);
}

function numberValue(id) {
  return Number($(id).value);
}

function outputPayload() {
  return {
    output_dir: $("output-dir").value,
    filename: $("output-file").value,
    auto_timestamp_suffix: $("output-auto").checked,
  };
}

function storeOutput(mode = currentMode) {
  outputsByMode[mode] = outputPayload();
}

function applyOutput(mode) {
  const output = outputsByMode[mode] || {};
  $("output-dir").value = output.output_dir || output.dir || "";
  $("output-file").value = output.filename || "";
  $("output-auto").checked = Boolean(output.auto_timestamp_suffix ?? true);
}

function selectMode(mode) {
  storeOutput();
  currentMode = mode;
  document.querySelectorAll(".tab").forEach((tab) => tab.classList.toggle("active", tab.dataset.mode === mode));
  document.querySelectorAll(".settings-form").forEach((form) => form.classList.add("hidden"));
  $(`settings-${mode}`).classList.remove("hidden");
  $("settings-title").textContent = titles[mode];
  applyOutput(mode);
  renderRows();
  drawPlot();
}

function collectSettings(mode) {
  if (mode === "signal_monitor") {
    return { interval_s: numberValue("signal-interval"), n_points: numberValue("signal-points") };
  }
  if (mode === "trkr") {
    return {
      coordinate: "measurement",
      scan: rangePayload("trkr"),
      wait_s: numberValue("trkr-wait"),
      return_to_zero: $("trkr-return").checked,
    };
  }
  if (mode === "srkr") {
    return {
      coordinate: "measurement",
      scan: { axis: $("srkr-axis").value, ...rangePayload("srkr") },
      wait_s: numberValue("srkr-wait"),
      return_to_zero: $("srkr-return").checked,
    };
  }
  if (mode === "strkr") {
    return {
      scan: {
        fast_axis: $("strkr-fast").value,
        slow_axis: $("strkr-slow").value,
        ranges: {
          t: rangePayload("strkr-t"),
          x: rangePayload("strkr-x"),
          y: rangePayload("strkr-y"),
        },
      },
      wait_s: numberValue("strkr-wait"),
      return_to_zero: {
        fast_axis: $("strkr-return-fast").checked,
        slow_axis: $("strkr-return-slow").checked,
      },
    };
  }
  return {
    scan: {
      fast_axis: $("srkr2d-fast").value,
      slow_axis: $("srkr2d-slow").value,
      ranges: {
        x: rangePayload("srkr2d-x"),
        y: rangePayload("srkr2d-y"),
      },
    },
    wait_s: numberValue("srkr2d-wait"),
    return_to_zero: {
      fast_axis: $("srkr2d-return-fast").checked,
      slow_axis: $("srkr2d-return-slow").checked,
    },
  };
}

function rangePayload(prefix) {
  return {
    min: numberValue(`${prefix}-min`),
    max: numberValue(`${prefix}-max`),
    step: numberValue(`${prefix}-step`),
  };
}

function renderDevices() {
  const instruments = state.defaults?.instruments || {};
  const connected = state.connected || {};
  const container = $("device-list");
  container.innerHTML = "";
  Object.entries(instruments).forEach(([kind, devices]) => {
    Object.entries(devices || {}).forEach(([key, config]) => {
      const ref = `${kind}.${key}`;
      const card = document.createElement("div");
      card.className = "device-card";
      card.innerHTML = `
        <div class="device-head">
          <span class="device-name">${escapeHtml(ref)}</span>
          <span class="status-dot ${connected[ref] ? "on" : ""}">${connected[ref] ? "connected" : "offline"}</span>
        </div>
        <pre class="mini-json">${escapeHtml(JSON.stringify(config, null, 2))}</pre>
        <div class="button-row">
          <button data-action="connect-device" data-ref="${escapeAttr(ref)}">Connect</button>
          <button data-action="disconnect-device" data-ref="${escapeAttr(ref)}">Disconnect</button>
          ${kind === "delay_stage" ? `<button data-action="init-delay" data-ref="${escapeAttr(ref)}">Initialize</button>` : ""}
          ${kind === "scanner" ? `<button data-action="init-scanner" data-axis="${escapeAttr(scannerAxis(key, config))}">Initialize</button>` : ""}
        </div>`;
      container.appendChild(card);
    });
  });
}

function renderAxisPanels() {
  const container = $("axis-panels");
  const live = state.live_status?.position || {};
  const zero = state.config?.measurements?.move_abs?.zero || {};
  container.innerHTML = "";
  [
    ["t", "ps", live.t_ps, zero.t_ps || 0],
    ["x", "um", live.x_um, zero.x_um || 0],
    ["y", "um", live.y_um, zero.y_um || 0],
  ].forEach(([axis, unit, absolute, origin]) => {
    const corrected = isFiniteNumber(absolute) ? Number(absolute) - Number(origin) : null;
    const card = document.createElement("div");
    card.className = "axis-card";
    card.innerHTML = `
      <h2>${axis.toUpperCase()}</h2>
      <dl class="run-status">
        <div><dt>live</dt><dd>${formatValue(absolute)} ${unit}</dd></div>
        <div><dt>origin</dt><dd>${formatValue(origin)} ${unit}</dd></div>
        <div><dt>cor</dt><dd>${formatValue(corrected)} ${unit}</dd></div>
      </dl>
      <label class="field"><span>absolute</span><input id="move-${axis}-abs" type="number" step="0.1" /></label>
      <button data-action="move-abs" data-axis="${axis}" data-source="abs">Move absolute</button>
      <label class="field"><span>corrected</span><input id="move-${axis}-cor" type="number" step="0.1" /></label>
      <button data-action="move-abs" data-axis="${axis}" data-source="cor">Move corrected</button>`;
    container.appendChild(card);
  });
}

function renderRunStatus() {
  const job = state.job || {};
  $("run-status").textContent = job.status || "-";
  $("run-point").textContent = job.point || "-";
  $("run-output").textContent = job.output_path || state.last_output_path?.[currentMode] || "-";
  $("start-run").disabled = Boolean(job.running || state.active_operation);
  $("stop-run").disabled = !job.running;
}

function renderRows() {
  const table = $("rows-table");
  const rows = (state.rows?.[currentMode] || []).slice(-150);
  if (!rows.length) {
    table.innerHTML = "<tbody><tr><td>No rows.</td></tr></tbody>";
    return;
  }
  const keys = Object.keys(rows[0]).filter((key) => rows.some((row) => row[key] !== null && row[key] !== undefined));
  table.innerHTML = `
    <thead><tr>${keys.map((key) => `<th>${escapeHtml(key)}</th>`).join("")}</tr></thead>
    <tbody>${rows
      .map((row) => `<tr>${keys.map((key) => `<td>${escapeHtml(formatCell(row[key]))}</td>`).join("")}</tr>`)
      .join("")}</tbody>`;
}

function renderSnapshot() {
  const connected = state.connected || {};
  const live = state.live_status || {};
  const position = live.position || {};
  const signal = live.signal || {};
  const rows = [
    ["Config", state.config_path],
    ["Connected", Object.entries(connected).filter(([, value]) => value).map(([key]) => key).join(", ") || "-"],
    ["t", formatValue(position.t_ps)],
    ["x", formatValue(position.x_um)],
    ["y", formatValue(position.y_um)],
    ["X", formatValue(signal.X ?? signal.X_V)],
    ["Y", formatValue(signal.Y ?? signal.Y_V)],
    ["R", formatValue(signal.R ?? signal.R_V)],
    ["Theta", formatValue(signal.Theta ?? signal.Theta_deg)],
    ["Rows", state.row_counts?.[currentMode] || 0],
  ];
  $("snapshot-table").innerHTML = `<tbody>${rows
    .map(([key, value]) => `<tr><td>${escapeHtml(key)}</td><td>${escapeHtml(String(value ?? "-"))}</td></tr>`)
    .join("")}</tbody>`;
}

function renderLogs() {
  $("log-output").textContent = (state.logs || []).slice(-180).join("\n");
  $("log-output").scrollTop = $("log-output").scrollHeight;
}

function drawPlot() {
  const canvas = $("plot");
  const ctx = canvas.getContext("2d");
  const rows = state?.rows?.[currentMode] || [];
  const width = canvas.width;
  const height = canvas.height;
  ctx.clearRect(0, 0, width, height);
  ctx.fillStyle = "#ffffff";
  ctx.fillRect(0, 0, width, height);
  ctx.strokeStyle = "#d7dee2";
  ctx.lineWidth = 1;
  for (let i = 0; i <= 5; i += 1) {
    const y = 24 + ((height - 48) * i) / 5;
    ctx.beginPath();
    ctx.moveTo(44, y);
    ctx.lineTo(width - 18, y);
    ctx.stroke();
  }
  ctx.fillStyle = "#5e6b72";
  ctx.fillText(`${titles[currentMode]} | X_V`, 48, 18);
  if (!rows.length) {
    ctx.fillText("No data", 48, 48);
    return;
  }
  const points = rows
    .map((row, index) => ({ x: xValue(row, index), y: Number(row.X_V) }))
    .filter((point) => Number.isFinite(point.x) && Number.isFinite(point.y));
  if (!points.length) return;
  const minX = Math.min(...points.map((p) => p.x));
  const maxX = Math.max(...points.map((p) => p.x));
  const minY = Math.min(...points.map((p) => p.y));
  const maxY = Math.max(...points.map((p) => p.y));
  const spanX = maxX - minX || 1;
  const spanY = maxY - minY || 1;
  const sx = (x) => 44 + ((x - minX) / spanX) * (width - 70);
  const sy = (y) => height - 24 - ((y - minY) / spanY) * (height - 54);
  ctx.strokeStyle = "#137a7f";
  ctx.lineWidth = 2;
  ctx.beginPath();
  points.forEach((point, index) => {
    const x = sx(point.x);
    const y = sy(point.y);
    if (index === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  });
  ctx.stroke();
  ctx.fillStyle = "#8b5d13";
  points.slice(-1).forEach((point) => {
    ctx.beginPath();
    ctx.arc(sx(point.x), sy(point.y), 4, 0, Math.PI * 2);
    ctx.fill();
  });
}

function xValue(row, index) {
  if (currentMode === "signal_monitor") return Number(row.elapsed_s ?? row.target_elapsed_s ?? index);
  if (currentMode === "trkr") return Number(row.t_cor_ps ?? row.target_t_cor_ps ?? index);
  if (currentMode === "srkr") return Number(row.x_cor_um ?? row.y_cor_um ?? row.target_x_cor_um ?? row.target_y_cor_um ?? index);
  return index + 1;
}

function scannerAxis(key, config) {
  const normalized = String(key).toLowerCase();
  if (normalized.endsWith("y") || String(config?.axis).toLowerCase() === "y") return "y";
  return "x";
}

function liveAbsolute(axis) {
  const position = state?.live_status?.position || {};
  if (axis === "t") return Number(position.t_ps);
  if (axis === "x") return Number(position.x_um);
  return Number(position.y_um);
}

function zeroFor(axis) {
  const zero = state?.config?.measurements?.move_abs?.zero || {};
  if (axis === "t") return Number(zero.t_ps || 0);
  if (axis === "x") return Number(zero.x_um || 0);
  return Number(zero.y_um || 0);
}

function isFiniteNumber(value) {
  return Number.isFinite(Number(value));
}

function formatValue(value) {
  if (!isFiniteNumber(value)) return "-";
  return Number(value).toFixed(6).replace(/\.?0+$/, "");
}

function formatCell(value) {
  if (value === null || value === undefined) return "";
  if (typeof value === "number") return Number.isFinite(value) ? String(Number(value.toPrecision(8))) : String(value);
  return String(value);
}

function appendLocalLog(message) {
  const log = $("log-output");
  log.textContent = `${log.textContent}\n${message}`.trim();
}

function escapeHtml(value) {
  return String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

function escapeAttr(value) {
  return escapeHtml(value);
}

function buildRangeControls() {
  document.querySelectorAll(".axis-range").forEach((container) => {
    const prefix = container.dataset.prefix;
    const axis = container.dataset.axis;
    const id = `${prefix}-${axis}`;
    const unit = axis === "t" ? "ps" : "um";
    container.innerHTML = `
      <div class="range-title">${axis.toUpperCase()} range (${unit})</div>
      <div class="three">
        <label class="field"><span>min</span><input id="${id}-min" type="number" step="0.1" /></label>
        <label class="field"><span>max</span><input id="${id}-max" type="number" step="0.1" /></label>
        <label class="field"><span>step</span><input id="${id}-step" type="number" step="0.1" /></label>
      </div>`;
  });
}

function wireEvents() {
  document.querySelectorAll(".tab").forEach((tab) => tab.addEventListener("click", () => selectMode(tab.dataset.mode)));
  $("config-json").addEventListener("focus", () => {
    editingConfig = true;
  });
  $("config-json").addEventListener("blur", () => {
    editingConfig = false;
  });
  $("load-config").addEventListener("click", () => runAction(() => api("/api/config/load", { path: $("config-path").value })));
  $("save-config").addEventListener("click", () => runAction(() => api("/api/config/save", { path: $("config-path").value })));
  $("save-config-json").addEventListener("click", () =>
    runAction(() => api("/api/config/save", { path: $("config-path").value, config: JSON.parse($("config-json").value) })),
  );
  $("connect-all").addEventListener("click", () => runAction(() => api("/api/devices/connect-all", {})));
  $("disconnect-all").addEventListener("click", () => runAction(() => api("/api/devices/disconnect-all", {})));
  $("read-live").addEventListener("click", () => runAction(() => api("/api/live/read", {})));
  $("start-run").addEventListener("click", () => {
    storeOutput();
    runAction(() =>
      api("/api/measurements/start", {
        measurement: currentMode,
        settings: collectSettings(currentMode),
        output: outputPayload(),
      }),
    );
  });
  $("stop-run").addEventListener("click", () => runAction(() => api("/api/measurements/stop", {})));
  $("save-rows").addEventListener("click", () => {
    storeOutput();
    runAction(() => api("/api/measurements/save", { measurement: currentMode, output: outputPayload() }));
  });
  document.body.addEventListener("click", (event) => {
    const button = event.target.closest("button[data-action]");
    if (!button) return;
    const action = button.dataset.action;
    if (action === "connect-device") runAction(() => api("/api/devices/connect", { ref: button.dataset.ref }));
    if (action === "disconnect-device") runAction(() => api("/api/devices/disconnect", { ref: button.dataset.ref }));
    if (action === "init-delay") runAction(() => api("/api/devices/initialize-delay-stage", { ref: button.dataset.ref }));
    if (action === "init-scanner") runAction(() => api("/api/devices/initialize-scanner", { axis: button.dataset.axis }));
    if (action === "move-abs") {
      const axis = button.dataset.axis;
      const source = button.dataset.source;
      const raw = Number($(`move-${axis}-${source}`).value);
      const value = source === "cor" ? zeroFor(axis) + raw : raw;
      runAction(() => api("/api/move", { axis, value, coordinate: "measurement" }));
    }
  });
}

async function runAction(fn) {
  try {
    await fn();
  } catch (error) {
    appendLocalLog(`error: ${error.message}`);
    alert(error.message);
  }
}

buildRangeControls();
wireEvents();
selectMode(currentMode);
refresh();
setInterval(refresh, 1200);
