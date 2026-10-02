const $ = (selector) => document.querySelector(selector);
const elements = {
  runtimeDot: $("#runtime-dot"),
  runtimeLabel: $("#runtime-label"),
  input: $("#file-input"),
  dropzone: $("#dropzone"),
  browse: $("#browse-file"),
  clear: $("#clear-file"),
  fileSummary: $("#file-summary"),
  fileName: $("#file-name"),
  fileMeta: $("#file-meta"),
  inputFormat: $("#input-format"),
  comparePanel: $("#compare-panel"),
  compareToolbar: $(".compare-toolbar"),
  compareStage: $("#compare-stage"),
  compareDivider: $("#compare-divider"),
  zoomLayer: $("#zoom-layer"),
  zoomOut: $("#zoom-out"),
  zoomIn: $("#zoom-in"),
  zoomFit: $("#zoom-fit"),
  zoomOutput: $("#zoom-output"),
  beforePreview: $("#before-preview"),
  afterPreview: $("#after-preview"),
  compareOutput: $("#compare-output"),
  target: $("#target-format"),
  capability: $("#capability-text"),
  capabilityIndicator: $(".capability-indicator"),
  preset: $("#preset"),
  presetHint: $("#preset-hint"),
  compression: $("#compression"),
  quality: $("#quality"),
  qualityOutput: $("#quality-output"),
  effort: $("#effort"),
  effortOutput: $("#effort-output"),
  chromaBlock: $("#chroma-block"),
  chroma: $("#chroma"),
  bitdepth: $("#bitdepth"),
  pngFilterBlock: $("#png-filter-block"),
  pngFilter: $("#png-filter"),
  jxlControls: $("#jxl-controls"),
  jxlNoise: $("#jxl-noise"),
  jxlGaborish: $("#jxl-gaborish"),
  keepExif: $("#keep-exif"),
  keepXmp: $("#keep-xmp"),
  autoRotate: $("#auto-rotate"),
  strictMetadata: $("#strict-metadata"),
  memoryLimit: $("#memory-limit"),
  resizeEdge: $("#resize-edge"),
  crop: $("#crop"),
  colorOverride: $("#color-override"),
  demosaic: $("#demosaic"),
  convert: $("#convert-button"),
  error: $("#error-message"),
  status: $("#status-message"),
  result: $("#result-panel"),
  resultMeta: $("#result-meta"),
  download: $("#download-result"),
};

const formats = {
  png: { extension: "png", label: "PNG" },
  jpeg: { extension: "jpg", label: "JPEG" },
  tiff: { extension: "tiff", label: "TIFF" },
  bmp: { extension: "bmp", label: "BMP" },
  webp: { extension: "webp", label: "WebP" },
  avif: { extension: "avif", label: "AVIF" },
  jxl: { extension: "jxl", label: "JPEG XL" },
};

let wasm;
let selectedFile;
let selectedBytes;
let detectedFormat;
let capability = "never";
let outputBytes;
let outputName;
let beforePreviewUrl;
let afterPreviewUrl;
let zoomLevel = 1;
const MIN_ZOOM = 0.1;
const MAX_ZOOM = 64;
let panX = 0;
let panY = 0;
let activeDrag;
let spaceHeld = false;

function setStatus(message = "") {
  elements.status.textContent = message;
}

function setError(error) {
  if (!error) {
    elements.error.hidden = true;
    elements.error.textContent = "";
    return;
  }
  const code = typeof error === "object" && error.code ? `${error.code}: ` : "";
  const message = typeof error === "object" && error.message ? error.message : String(error);
  elements.error.textContent = `${code}${message}`;
  elements.error.hidden = false;
}

function formatSize(bytes) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
}

function updateCapability() {
  if (!wasm) return;
  const target = elements.target.value;
  try {
    setError(null);
    capability = wasm.losslessCapability(target);
    elements.capability.textContent = {
      always: "This format is lossless-only",
      never: "Lossy only · no true lossless encoder",
      configurable: "Lossy and lossless supported",
    }[capability];
    elements.capabilityIndicator.className = `capability-indicator ${capability}`;
  } catch (error) {
    capability = "never";
    elements.capability.textContent = "Encoder unavailable for this target";
    elements.capabilityIndicator.className = "capability-indicator never";
    setError(error);
  }
  if (capability === "always") elements.compression.value = "lossless";
  if (capability === "never") elements.compression.value = "lossy";
  elements.compression.disabled = capability !== "configurable";
  updateControlAvailability();
  updatePresetAvailability();
}

function updateControlAvailability() {
  const target = elements.target.value;
  const lossless = capability === "always" || elements.compression.value === "lossless";
  elements.quality.disabled = lossless;
  elements.qualityOutput.textContent = elements.quality.value;
  elements.chromaBlock.hidden = target !== "jpeg";
  elements.chroma.disabled = target !== "jpeg" || lossless;
  elements.bitdepth.querySelector('option[value="Ten"]').disabled = true;
  elements.bitdepth.querySelector('option[value="Twelve"]').disabled = true;
  elements.bitdepth.querySelector('option[value="Sixteen"]').disabled = !["png", "tiff"].includes(target);
  if (!["png", "tiff"].includes(target) && elements.bitdepth.value === "Sixteen") {
    elements.bitdepth.value = "Eight";
  }
  elements.pngFilterBlock.hidden = target !== "png";
  elements.pngFilter.disabled = target !== "png";
  elements.jxlControls.hidden = target !== "jxl";
  elements.jxlNoise.disabled = target !== "jxl" || lossless;
  elements.jxlGaborish.disabled = target !== "jxl" || lossless;

  const canKeepExif = ["png", "webp", "avif"].includes(target);
  const canKeepXmp = ["webp", "avif"].includes(target);
  elements.keepExif.disabled = !canKeepExif;
  elements.keepXmp.disabled = !canKeepXmp;
  if (!canKeepExif) elements.keepExif.checked = false;
  if (!canKeepXmp) elements.keepXmp.checked = false;
  elements.demosaic.value = "Fast";
}

function updatePresetAvailability() {
  const target = elements.target.value;
  const available = {
    "web-optimized": ["webp", "avif"].includes(target),
    "max-quality-archive": ["png", "tiff", "bmp", "webp", "jxl"].includes(target),
    "social-media": target === "jpeg",
  };
  for (const option of elements.preset.options) {
    if (option.value !== "none") option.disabled = !available[option.value];
  }
  if (elements.preset.value !== "none" && !available[elements.preset.value]) {
    elements.preset.value = "none";
    elements.presetHint.textContent = "That preset is not available for this target.";
  } else {
    elements.presetHint.textContent = "Presets set Core encode options; all use the same conversion path.";
  }
}

function applyPreset() {
  const target = elements.target.value;
  const preset = elements.preset.value;
  elements.keepExif.checked = false;
  elements.keepXmp.checked = false;
  elements.resizeEdge.value = "";

  if (preset === "web-optimized") {
    elements.compression.value = "lossy";
    elements.quality.value = "75";
    elements.bitdepth.value = "Eight";
  } else if (preset === "max-quality-archive") {
    elements.compression.value = "lossless";
    elements.quality.value = "75";
    elements.effort.value = "10";
    elements.bitdepth.value = ["png", "tiff"].includes(target) ? "Sixteen" : "Eight";
    elements.keepExif.checked = ["png", "webp"].includes(target);
    elements.keepXmp.checked = target === "webp";
  } else if (preset === "social-media") {
    elements.compression.value = "lossy";
    elements.quality.value = "82";
    elements.bitdepth.value = "Eight";
    elements.resizeEdge.value = "1350";
  }
  updateCapability();
}

async function selectFile(file) {
  if (!file) return;
  clearPreviews();
  setError(null);
  elements.result.hidden = true;
  elements.compareStage.classList.remove("has-result");
  elements.compareStage.setAttribute("role", "img");
  elements.compareStage.setAttribute("tabindex", "-1");
  elements.compareStage.setAttribute("aria-disabled", "true");
  resetZoom();
  elements.dropzone.hidden = true;
  setStatus("Reading file…");
  selectedFile = file;
  selectedBytes = new Uint8Array(await file.arrayBuffer());
  detectedFormat = wasm?.probeFormat(selectedBytes) ?? null;
  elements.fileName.textContent = file.name;
  elements.fileMeta.textContent = `${formatSize(file.size)} · ${file.type || "image file"}`;
  elements.inputFormat.textContent = detectedFormat || "unknown";
  elements.fileSummary.hidden = false;
  elements.clear.disabled = false;
  elements.convert.disabled = !detectedFormat || !wasm;
  if (detectedFormat) {
    try {
      beforePreviewUrl = createPreviewUrl(
        wasm.convert(selectedBytes, "png", decodeOptions(), previewEncodeOptions()),
      );
      elements.beforePreview.src = beforePreviewUrl;
      elements.comparePanel.hidden = false;
      updateComparison(50);
      setStatus(`Detected ${detectedFormat.toUpperCase()} input.`);
    } catch (error) {
      elements.comparePanel.hidden = true;
      setStatus("");
      setError(error);
    }
  } else {
    elements.comparePanel.hidden = true;
    setStatus("");
    setError({ code: "unsupported_format", message: "Core could not identify this image format." });
  }
}

function encodeOptions() {
  const compression = elements.compression.value === "lossless"
    ? "Lossless"
    : { Lossy: { quality: Number(elements.quality.value) } };
  const cropText = elements.crop.value.trim();
  let crop = null;
  if (cropText) {
    const values = cropText.split(/[\s,]+/).filter(Boolean).map(Number);
    if (values.length !== 4 || values.some((value) => !Number.isInteger(value) || value < 0)) {
      throw { code: "invalid_options", message: "Crop must be four non-negative integers: x, y, width, height." };
    }
    crop = { x: values[0], y: values[1], width: values[2], height: values[3] };
  }
  return {
    compression,
    bit_depth: elements.bitdepth.value,
    chroma_subsampling: elements.chroma.value || null,
    png_filter: elements.pngFilter.value || null,
    effort: Number(elements.effort.value),
    jxl_noise_synthesis: elements.jxlNoise.checked,
    jxl_gaborish: elements.jxlGaborish.checked,
    metadata_retention: {
      exif: elements.keepExif.checked,
      iptc: false,
      xmp: elements.keepXmp.checked,
    },
    resize_long_edge: elements.resizeEdge.value ? Number(elements.resizeEdge.value) : null,
    crop,
  };
}

function decodeOptions() {
  return {
    strict_metadata: elements.strictMetadata.checked,
    color_space_override: elements.colorOverride.value || null,
    auto_rotate: elements.autoRotate.checked,
    demosaic_quality: elements.demosaic.value,
    memory_limit_mb: elements.memoryLimit.value ? Number(elements.memoryLimit.value) : null,
  };
}

async function convert() {
  if (!selectedBytes || !wasm) return;
  clearPreviews();
  setError(null);
  elements.convert.disabled = true;
  elements.result.hidden = true;
  elements.comparePanel.hidden = true;
  setStatus("Converting locally…");
  try {
    const target = elements.target.value;
    const decode = decodeOptions();
    const options = encodeOptions();
    const result = wasm.convert(selectedBytes, target, decode, options);
    outputBytes = result instanceof Uint8Array ? result : new Uint8Array(result);
    outputName = `${selectedFile.name.replace(/\.[^.]+$/, "")}.${formats[target].extension}`;
    elements.resultMeta.textContent = `${formats[target].label} · ${formatSize(outputBytes.byteLength)}`;
    elements.result.hidden = false;
    try {
      beforePreviewUrl = createPreviewUrl(
        wasm.convert(selectedBytes, "png", decode, previewEncodeOptions()),
      );
      afterPreviewUrl = createPreviewUrl(
        wasm.convert(
          outputBytes,
          "png",
          { auto_rotate: true, memory_limit_mb: decode.memory_limit_mb },
          previewEncodeOptions(),
        ),
      );
      elements.beforePreview.src = beforePreviewUrl;
      elements.afterPreview.src = afterPreviewUrl;
      elements.comparePanel.hidden = false;
      elements.comparePanel.classList.add("comparing");
      elements.compareStage.classList.add("has-result");
      elements.compareStage.setAttribute("role", "slider");
      elements.compareStage.setAttribute("tabindex", "0");
      elements.compareStage.setAttribute("aria-disabled", "false");
      resetZoom();
      updateComparison();
      setStatus("Ready. Drag the divider to compare the converted result.");
    } catch (previewError) {
      clearPreviews();
      elements.comparePanel.hidden = true;
      setStatus(`Converted. Preview unavailable: ${errorMessage(previewError)}`);
    }
  } catch (error) {
    setStatus("");
    setError(error);
  } finally {
    elements.convert.disabled = !detectedFormat;
  }
}

function previewEncodeOptions() {
  return {
    compression: "Lossless",
    bit_depth: "Eight",
    metadata_retention: { exif: false, iptc: false, xmp: false },
  };
}

function createPreviewUrl(bytes) {
  const preview = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  return URL.createObjectURL(new Blob([preview], { type: "image/png" }));
}

function clearPreviews() {
  if (beforePreviewUrl) URL.revokeObjectURL(beforePreviewUrl);
  if (afterPreviewUrl) URL.revokeObjectURL(afterPreviewUrl);
  beforePreviewUrl = undefined;
  afterPreviewUrl = undefined;
  elements.beforePreview.removeAttribute("src");
  elements.afterPreview.removeAttribute("src");
  elements.compareStage.classList.remove("has-result");
  elements.comparePanel.classList.remove("comparing");
}

function errorMessage(error) {
  return typeof error === "object" && error?.message ? error.message : String(error);
}

function updateComparison(beforePercent = Number(elements.compareStage.dataset.split || 50)) {
  beforePercent = Math.max(0, Math.min(100, Math.round(beforePercent)));
  const afterPercent = 100 - beforePercent;
  elements.compareStage.dataset.split = String(beforePercent);
  elements.compareStage.style.setProperty("--split", `${beforePercent}%`);
  elements.compareStage.setAttribute("aria-valuenow", String(beforePercent));
  elements.compareStage.setAttribute(
    "aria-valuetext",
    `${beforePercent} percent before, ${afterPercent} percent after`,
  );
  elements.compareOutput.textContent = `${beforePercent}% / ${afterPercent}%`;
  updateDividerPosition();
}

function moveComparisonTo(clientX) {
  const bounds = elements.compareStage.getBoundingClientRect();
  if (bounds.width === 0) return;
  const centerX = bounds.left + bounds.width / 2 + panX;
  updateComparison(50 + ((clientX - centerX) / (bounds.width * zoomLevel)) * 100);
}

function clampPan() {
  const bounds = elements.compareStage.getBoundingClientRect();
  if (zoomLevel <= 1) {
    panX = 0;
    panY = 0;
    return;
  }
  const maxPanX = ((zoomLevel - 1) * bounds.width) / 2;
  const maxPanY = ((zoomLevel - 1) * bounds.height) / 2;
  panX = Math.max(-maxPanX, Math.min(maxPanX, panX));
  panY = Math.max(-maxPanY, Math.min(maxPanY, panY));
}

function updateZoom() {
  clampPan();
  elements.zoomLayer.style.setProperty("--zoom", String(zoomLevel));
  elements.zoomLayer.style.setProperty("--pan-x", `${panX}px`);
  elements.zoomLayer.style.setProperty("--pan-y", `${panY}px`);
  elements.zoomOutput.textContent = `${Math.round(zoomLevel * 100)}%`;
  elements.zoomOut.disabled = zoomLevel <= MIN_ZOOM;
  elements.zoomIn.disabled = zoomLevel >= MAX_ZOOM;
  elements.compareStage.classList.toggle("panning", Boolean(activeDrag?.mode === "pan"));
  updateDividerPosition();
}

function updateDividerPosition() {
  if (!elements.compareStage || !elements.compareDivider) return;
  const bounds = elements.compareStage.getBoundingClientRect();
  if (bounds.width === 0) return;
  const split = Number(elements.compareStage.dataset.split || 50) / 100;
  const dividerX = bounds.width / 2 + panX + (split - 0.5) * bounds.width * zoomLevel;
  elements.compareDivider.style.setProperty("--divider-x", `${dividerX}px`);
}

function setZoom(nextZoom, anchorX, anchorY) {
  const next = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, nextZoom));
  if (next === zoomLevel) return;
  const bounds = elements.compareStage.getBoundingClientRect();
  const centerX = bounds.left + bounds.width / 2;
  const centerY = bounds.top + bounds.height / 2;
  const x = anchorX ?? centerX;
  const y = anchorY ?? centerY;
  const contentX = (x - centerX - panX) / zoomLevel;
  const contentY = (y - centerY - panY) / zoomLevel;
  panX = x - centerX - contentX * next;
  panY = y - centerY - contentY * next;
  zoomLevel = next;
  updateZoom();
}

function resetZoom() {
  zoomLevel = 1;
  panX = 0;
  panY = 0;
  activeDrag = undefined;
  updateZoom();
}

function download() {
  if (!outputBytes) return;
  const blob = new Blob([outputBytes], { type: "application/octet-stream" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = outputName;
  link.click();
  URL.revokeObjectURL(url);
}

function clearFile() {
  clearPreviews();
  resetZoom();
  selectedFile = undefined;
  selectedBytes = undefined;
  detectedFormat = undefined;
  outputBytes = undefined;
  elements.input.value = "";
  elements.fileSummary.hidden = true;
  elements.dropzone.hidden = false;
  elements.comparePanel.hidden = true;
  elements.result.hidden = true;
  elements.clear.disabled = true;
  elements.convert.disabled = true;
  setError(null);
  setStatus("");
}

elements.browse.addEventListener("click", () => elements.input.click());
elements.dropzone.addEventListener("click", (event) => {
  if (event.target.closest("button")) return;
  elements.input.click();
});
elements.dropzone.addEventListener("keydown", (event) => {
  if (event.key === "Enter" || event.key === " ") {
    event.preventDefault();
    elements.input.click();
  }
});
elements.input.addEventListener("change", () => selectFile(elements.input.files?.[0]));
elements.dropzone.addEventListener("dragover", (event) => {
  event.preventDefault();
  elements.dropzone.classList.add("dragging");
});
elements.dropzone.addEventListener("dragleave", () => elements.dropzone.classList.remove("dragging"));
elements.dropzone.addEventListener("drop", (event) => {
  event.preventDefault();
  elements.dropzone.classList.remove("dragging");
  selectFile(event.dataTransfer?.files?.[0]);
});
elements.clear.addEventListener("click", clearFile);
elements.target.addEventListener("change", () => {
  elements.preset.value = "none";
  updateCapability();
});
elements.compression.addEventListener("change", updateControlAvailability);
elements.quality.addEventListener("input", () => { elements.qualityOutput.textContent = elements.quality.value; });
elements.effort.addEventListener("input", () => { elements.effortOutput.textContent = elements.effort.value; });
elements.preset.addEventListener("change", applyPreset);
elements.convert.addEventListener("click", convert);
elements.download.addEventListener("click", download);
elements.compareStage.addEventListener("pointerdown", (event) => {
  if (!elements.compareStage.classList.contains("has-result")) return;
  event.preventDefault();
  elements.compareStage.setPointerCapture(event.pointerId);
  if ((spaceHeld || event.shiftKey) && zoomLevel > 1) {
    activeDrag = {
      mode: "pan",
      x: event.clientX,
      y: event.clientY,
      panX,
      panY,
    };
    updateZoom();
  } else {
    activeDrag = { mode: "compare" };
    moveComparisonTo(event.clientX);
  }
});
elements.compareStage.addEventListener("pointermove", (event) => {
  if (
    elements.compareStage.classList.contains("has-result") &&
    elements.compareStage.hasPointerCapture(event.pointerId)
  ) {
    event.preventDefault();
    if (activeDrag?.mode === "pan") {
      panX = activeDrag.panX + event.clientX - activeDrag.x;
      panY = activeDrag.panY + event.clientY - activeDrag.y;
      updateZoom();
    } else {
      moveComparisonTo(event.clientX);
    }
  }
});
elements.compareStage.addEventListener("pointerup", (event) => {
  if (elements.compareStage.hasPointerCapture(event.pointerId)) {
    elements.compareStage.releasePointerCapture(event.pointerId);
  }
  activeDrag = undefined;
  updateZoom();
});
elements.compareStage.addEventListener("pointercancel", () => {
  activeDrag = undefined;
  updateZoom();
});
elements.compareStage.addEventListener("wheel", (event) => {
  if (!elements.compareStage.classList.contains("has-result")) return;
  event.preventDefault();
  const delta = event.deltaMode === WheelEvent.DOM_DELTA_LINE
    ? event.deltaY * 16
    : event.deltaMode === WheelEvent.DOM_DELTA_PAGE
      ? event.deltaY * elements.compareStage.clientHeight
      : event.deltaY;
  const factor = Math.exp(-delta * 0.0015);
  setZoom(zoomLevel * factor, event.clientX, event.clientY);
}, { passive: false });
elements.compareStage.addEventListener("keydown", (event) => {
  if (!elements.compareStage.classList.contains("has-result")) return;
  const current = Number(elements.compareStage.dataset.split || 50);
  if (event.key === "ArrowLeft" || event.key === "ArrowDown") {
    event.preventDefault();
    updateComparison(current - 5);
  } else if (event.key === "ArrowRight" || event.key === "ArrowUp") {
    event.preventDefault();
    updateComparison(current + 5);
  } else if (event.key === "Home") {
    event.preventDefault();
    updateComparison(0);
  } else if (event.key === "End") {
    event.preventDefault();
    updateComparison(100);
  }
});
elements.zoomOut.addEventListener("click", () => setZoom(zoomLevel / 1.2));
elements.zoomIn.addEventListener("click", () => setZoom(zoomLevel * 1.2));
elements.zoomFit.addEventListener("click", resetZoom);
window.addEventListener("keydown", (event) => {
  const target = event.target;
  const interactive = target instanceof HTMLElement && (
    target.isContentEditable ||
    ["INPUT", "TEXTAREA", "SELECT", "BUTTON", "A"].includes(target.tagName) ||
    target.closest('[role="button"]')
  );
  if (event.code === "Space" && !interactive && !event.repeat) {
    spaceHeld = true;
    if (zoomLevel > 1 && elements.compareStage.classList.contains("has-result")) event.preventDefault();
  }
});
window.addEventListener("keyup", (event) => {
  if (event.code === "Space") spaceHeld = false;
});

async function initializeWasm() {
  try {
    const bindings = await import("../pkg/prok_wasm.js");
    await bindings.default();
    wasm = bindings;
    elements.runtimeDot.classList.add("ready");
    elements.runtimeLabel.textContent = `Core ${wasm.version()} ready`;
    updateCapability();
    elements.convert.disabled = !selectedBytes || !detectedFormat;
  } catch (error) {
    elements.runtimeDot.classList.add("failed");
    elements.runtimeLabel.textContent = "WASM package unavailable";
    setError({
      code: "wasm_unavailable",
      message: "Build the WASM package with npm run build:wasm, then reload this page.",
    });
    console.error(error);
  }
}

initializeWasm();