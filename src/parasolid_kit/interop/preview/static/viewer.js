"use strict";

const canvas = document.getElementById("viewer");
const selectionTitle = document.getElementById("selection-title");
const selectionDetails = document.getElementById("selection-details");
const entityList = document.getElementById("entity-list");
const visibleCount = document.getElementById("visible-count");
const pickKind = document.getElementById("pick-kind");
const bodyFilter = document.getElementById("body-filter");
const surfaceFilter = document.getElementById("surface-filter");
const diagnosticFilter = document.getElementById("diagnostic-filter");
const selfTestOutput = document.getElementById("self-test");

const state = {
  gl: null,
  manifest: null,
  primitives: [],
  selected: null,
  yaw: -0.7,
  pitch: 0.48,
  distance: 1,
  center: [0, 0, 0],
  radius: 1,
  dragging: false,
  moved: false,
  pointer: [0, 0],
  mainProgram: null,
  pickProgram: null,
  pickFramebuffer: null,
  pickTexture: null,
  pickDepth: null,
};

const vertexShader = `
  attribute vec3 aPosition;
  attribute vec3 aNormal;
  uniform mat4 uMvp;
  varying float vLight;
  void main() {
    gl_Position = uMvp * vec4(aPosition, 1.0);
    vec3 normal = normalize(aNormal);
    vLight = 0.42 + 0.58 * abs(dot(normal, normalize(vec3(0.4, -0.5, 0.75))));
  }
`;

const fragmentShader = `
  precision mediump float;
  uniform vec4 uColor;
  varying float vLight;
  void main() {
    gl_FragColor = vec4(uColor.rgb * vLight, uColor.a);
  }
`;

const pickVertexShader = `
  attribute vec3 aPosition;
  uniform mat4 uMvp;
  void main() {
    gl_Position = uMvp * vec4(aPosition, 1.0);
  }
`;

const pickFragmentShader = `
  precision mediump float;
  uniform vec3 uPickColor;
  void main() {
    gl_FragColor = vec4(uPickColor, 1.0);
  }
`;

function compileProgram(gl, vertexSource, fragmentSource) {
  const compile = (type, source) => {
    const shader = gl.createShader(type);
    gl.shaderSource(shader, source);
    gl.compileShader(shader);
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
      throw new Error(gl.getShaderInfoLog(shader) || "shader compilation failed");
    }
    return shader;
  };
  const program = gl.createProgram();
  gl.attachShader(program, compile(gl.VERTEX_SHADER, vertexSource));
  gl.attachShader(program, compile(gl.FRAGMENT_SHADER, fragmentSource));
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    throw new Error(gl.getProgramInfoLog(program) || "shader link failed");
  }
  return program;
}

function parseGlb(arrayBuffer) {
  const view = new DataView(arrayBuffer);
  if (view.byteLength < 28 || view.getUint32(0, true) !== 0x46546c67) {
    throw new Error("preview.glb has an invalid header");
  }
  if (view.getUint32(4, true) !== 2 || view.getUint32(8, true) !== view.byteLength) {
    throw new Error("preview.glb is not a complete GLB 2.0 document");
  }
  const jsonLength = view.getUint32(12, true);
  const jsonType = view.getUint32(16, true);
  if (jsonType !== 0x4e4f534a) {
    throw new Error("preview.glb does not start with a JSON chunk");
  }
  const jsonBytes = new Uint8Array(arrayBuffer, 20, jsonLength);
  const documentJson = JSON.parse(new TextDecoder().decode(jsonBytes));
  const binHeader = 20 + jsonLength;
  const binLength = view.getUint32(binHeader, true);
  const binType = view.getUint32(binHeader + 4, true);
  if (binType !== 0x004e4942 || binHeader + 8 + binLength !== view.byteLength) {
    throw new Error("preview.glb does not contain one complete BIN chunk");
  }
  return { document: documentJson, buffer: arrayBuffer, binOffset: binHeader + 8 };
}

function accessorData(glb, accessorIndex) {
  const accessor = glb.document.accessors[accessorIndex];
  const bufferView = glb.document.bufferViews[accessor.bufferView];
  const components = { SCALAR: 1, VEC2: 2, VEC3: 3, VEC4: 4 }[accessor.type];
  const constructors = { 5125: Uint32Array, 5126: Float32Array };
  const Constructor = constructors[accessor.componentType];
  if (!components || !Constructor || bufferView.byteStride) {
    throw new Error("preview.glb uses an unsupported accessor layout");
  }
  const bytesPerElement = Constructor.BYTES_PER_ELEMENT;
  const byteOffset =
    glb.binOffset + (bufferView.byteOffset || 0) + (accessor.byteOffset || 0);
  const length = accessor.count * components;
  if (byteOffset + length * bytesPerElement > glb.buffer.byteLength) {
    throw new Error("preview.glb accessor exceeds its embedded buffer");
  }
  return new Constructor(glb.buffer, byteOffset, length);
}

function uploadPrimitive(gl, glb, primitive, metadata) {
  const positionData = accessorData(glb, primitive.attributes.POSITION);
  const normalData = primitive.attributes.NORMAL === undefined
    ? null
    : accessorData(glb, primitive.attributes.NORMAL);
  const indexData = accessorData(glb, primitive.indices);
  const positionBuffer = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, positionBuffer);
  gl.bufferData(gl.ARRAY_BUFFER, positionData, gl.STATIC_DRAW);
  const normalBuffer = normalData ? gl.createBuffer() : null;
  if (normalBuffer) {
    gl.bindBuffer(gl.ARRAY_BUFFER, normalBuffer);
    gl.bufferData(gl.ARRAY_BUFFER, normalData, gl.STATIC_DRAW);
  }
  const indexBuffer = gl.createBuffer();
  gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, indexBuffer);
  gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, indexData, gl.STATIC_DRAW);
  return {
    metadata,
    mode: primitive.mode === 4 ? gl.TRIANGLES : gl.LINE_STRIP,
    positionBuffer,
    normalBuffer,
    indexBuffer,
    indexCount: indexData.length,
  };
}

function createPickTarget(gl) {
  if (state.pickTexture) {
    gl.deleteTexture(state.pickTexture);
    gl.deleteRenderbuffer(state.pickDepth);
    gl.deleteFramebuffer(state.pickFramebuffer);
  }
  state.pickTexture = gl.createTexture();
  gl.bindTexture(gl.TEXTURE_2D, state.pickTexture);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
  gl.texImage2D(
    gl.TEXTURE_2D,
    0,
    gl.RGBA,
    canvas.width,
    canvas.height,
    0,
    gl.RGBA,
    gl.UNSIGNED_BYTE,
    null,
  );
  state.pickDepth = gl.createRenderbuffer();
  gl.bindRenderbuffer(gl.RENDERBUFFER, state.pickDepth);
  gl.renderbufferStorage(gl.RENDERBUFFER, gl.DEPTH_COMPONENT16, canvas.width, canvas.height);
  state.pickFramebuffer = gl.createFramebuffer();
  gl.bindFramebuffer(gl.FRAMEBUFFER, state.pickFramebuffer);
  gl.framebufferTexture2D(
    gl.FRAMEBUFFER,
    gl.COLOR_ATTACHMENT0,
    gl.TEXTURE_2D,
    state.pickTexture,
    0,
  );
  gl.framebufferRenderbuffer(
    gl.FRAMEBUFFER,
    gl.DEPTH_ATTACHMENT,
    gl.RENDERBUFFER,
    state.pickDepth,
  );
  if (gl.checkFramebufferStatus(gl.FRAMEBUFFER) !== gl.FRAMEBUFFER_COMPLETE) {
    throw new Error("browser could not create the picking framebuffer");
  }
  gl.bindFramebuffer(gl.FRAMEBUFFER, null);
}

function resize() {
  const ratio = Math.min(window.devicePixelRatio || 1, 2);
  const width = Math.max(1, Math.round(canvas.clientWidth * ratio));
  const height = Math.max(1, Math.round(canvas.clientHeight * ratio));
  if (canvas.width !== width || canvas.height !== height) {
    canvas.width = width;
    canvas.height = height;
    createPickTarget(state.gl);
  }
}

function perspective(fieldOfView, aspect, near, far) {
  const f = 1 / Math.tan(fieldOfView / 2);
  return [
    f / aspect, 0, 0, 0,
    0, f, 0, 0,
    0, 0, (far + near) / (near - far), -1,
    0, 0, (2 * far * near) / (near - far), 0,
  ];
}

function normalize(vector) {
  const length = Math.hypot(vector[0], vector[1], vector[2]) || 1;
  return vector.map((value) => value / length);
}

function subtract(left, right) {
  return left.map((value, index) => value - right[index]);
}

function cross(left, right) {
  return [
    left[1] * right[2] - left[2] * right[1],
    left[2] * right[0] - left[0] * right[2],
    left[0] * right[1] - left[1] * right[0],
  ];
}

function dot(left, right) {
  return left[0] * right[0] + left[1] * right[1] + left[2] * right[2];
}

function lookAt(eye, center, up) {
  const z = normalize(subtract(eye, center));
  const x = normalize(cross(up, z));
  const y = cross(z, x);
  return [
    x[0], y[0], z[0], 0,
    x[1], y[1], z[1], 0,
    x[2], y[2], z[2], 0,
    -dot(x, eye), -dot(y, eye), -dot(z, eye), 1,
  ];
}

function multiply(left, right) {
  const result = new Array(16).fill(0);
  for (let column = 0; column < 4; column += 1) {
    for (let row = 0; row < 4; row += 1) {
      for (let index = 0; index < 4; index += 1) {
        result[column * 4 + row] += left[index * 4 + row] * right[column * 4 + index];
      }
    }
  }
  return result;
}

function viewProjection() {
  const horizontal = Math.cos(state.pitch);
  const eye = [
    state.center[0] + state.distance * horizontal * Math.cos(state.yaw),
    state.center[1] + state.distance * horizontal * Math.sin(state.yaw),
    state.center[2] + state.distance * Math.sin(state.pitch),
  ];
  const view = lookAt(eye, state.center, [0, 0, 1]);
  const projection = perspective(
    Math.PI / 4,
    canvas.width / Math.max(1, canvas.height),
    Math.max(state.radius * 0.002, 0.0001),
    Math.max(state.radius * 20, state.distance + state.radius * 4),
  );
  return multiply(projection, view);
}

function visible(primitive) {
  const metadata = primitive.metadata;
  if (bodyFilter.value && !metadata.body_ids.map(String).includes(bodyFilter.value)) {
    return false;
  }
  if (surfaceFilter.value && !metadata.surface_kinds.includes(surfaceFilter.value)) {
    return false;
  }
  if (
    diagnosticFilter.value
    && !metadata.diagnostic_codes.includes(diagnosticFilter.value)
  ) {
    return false;
  }
  return true;
}

function bindPrimitive(gl, program, primitive, picking) {
  const position = gl.getAttribLocation(program, "aPosition");
  gl.bindBuffer(gl.ARRAY_BUFFER, primitive.positionBuffer);
  gl.enableVertexAttribArray(position);
  gl.vertexAttribPointer(position, 3, gl.FLOAT, false, 0, 0);
  const normal = gl.getAttribLocation(program, "aNormal");
  if (!picking && normal >= 0) {
    if (primitive.normalBuffer) {
      gl.bindBuffer(gl.ARRAY_BUFFER, primitive.normalBuffer);
      gl.enableVertexAttribArray(normal);
      gl.vertexAttribPointer(normal, 3, gl.FLOAT, false, 0, 0);
    } else {
      gl.disableVertexAttribArray(normal);
      gl.vertexAttrib3f(normal, 0, 0, 1);
    }
  }
  gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, primitive.indexBuffer);
}

function render(picking = false) {
  const gl = state.gl;
  resize();
  gl.bindFramebuffer(gl.FRAMEBUFFER, picking ? state.pickFramebuffer : null);
  gl.viewport(0, 0, canvas.width, canvas.height);
  gl.enable(gl.DEPTH_TEST);
  gl.disable(gl.BLEND);
  gl.clearColor(picking ? 0 : 0.035, picking ? 0 : 0.075, picking ? 0 : 0.105, 1);
  gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
  const program = picking ? state.pickProgram : state.mainProgram;
  gl.useProgram(program);
  gl.uniformMatrix4fv(gl.getUniformLocation(program, "uMvp"), false, viewProjection());
  const ordered = [
    ...state.primitives.filter((item) => item.metadata.kind === "face"),
    ...state.primitives.filter((item) => item.metadata.kind === "edge"),
  ];
  for (const primitive of ordered) {
    if (!visible(primitive)) {
      continue;
    }
    if (picking && primitive.metadata.kind !== pickKind.value) {
      continue;
    }
    bindPrimitive(gl, program, primitive, picking);
    if (picking) {
      const id = primitive.metadata.pick_id;
      gl.uniform3f(
        gl.getUniformLocation(program, "uPickColor"),
        (id & 255) / 255,
        ((id >> 8) & 255) / 255,
        ((id >> 16) & 255) / 255,
      );
      if (primitive.metadata.kind === "edge") {
        gl.lineWidth(7);
      }
    } else {
      const selected = state.selected === primitive;
      const color = primitive.metadata.kind === "face"
        ? (selected ? [1, 0.55, 0.16, 1] : [0.2, 0.58, 0.86, 1])
        : (selected ? [1, 0.34, 0.2, 1] : [0.04, 0.09, 0.13, 1]);
      gl.uniform4fv(gl.getUniformLocation(program, "uColor"), color);
      if (primitive.metadata.kind === "edge") {
        gl.lineWidth(selected ? 3 : 1);
      }
    }
    gl.drawElements(primitive.mode, primitive.indexCount, gl.UNSIGNED_INT, 0);
  }
  gl.bindFramebuffer(gl.FRAMEBUFFER, null);
}

function pickAt(x, y) {
  const gl = state.gl;
  render(true);
  gl.bindFramebuffer(gl.FRAMEBUFFER, state.pickFramebuffer);
  const pixel = new Uint8Array(4);
  gl.readPixels(x, y, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixel);
  gl.bindFramebuffer(gl.FRAMEBUFFER, null);
  const id = pixel[0] + (pixel[1] << 8) + (pixel[2] << 16);
  const primitive = state.primitives.find((item) => item.metadata.pick_id === id) || null;
  selectPrimitive(primitive);
  return primitive;
}

function addDetail(label, value) {
  const term = document.createElement("dt");
  term.textContent = label;
  const description = document.createElement("dd");
  description.textContent = value;
  selectionDetails.append(term, description);
}

function sourceSummary(metadata) {
  const source = metadata.source_entities[0];
  if (!source) {
    return "No source relation";
  }
  const range = source.byte_range;
  return `${source.key} · bytes ${range.start}–${range.end}`;
}

function selectPrimitive(primitive) {
  state.selected = primitive;
  selectionDetails.replaceChildren();
  if (!primitive) {
    selectionTitle.textContent = "Nothing selected";
  } else {
    const metadata = primitive.metadata;
    selectionTitle.textContent = `${metadata.kind} · ${metadata.target_key}`;
    addDetail("Parasolid faces", metadata.parasolid_face_ids.join(", ") || "—");
    addDetail("Parasolid edges", metadata.parasolid_edge_ids.join(", ") || "—");
    addDetail("Bodies", metadata.body_ids.join(", ") || "—");
    addDetail("Surface kinds", metadata.surface_kinds.join(", ") || "—");
    addDetail("Curve kinds", metadata.curve_kinds.join(", ") || "—");
    addDetail("Diagnostics", metadata.diagnostic_codes.join(", ") || "—");
    metadata.source_entities.forEach((source, index) => {
      addDetail(`Source ${index + 1}`, sourceSummary({ source_entities: [source] }));
      addDetail("Node", `${source.type_name} · node ID ${source.node_id ?? "none"}`);
    });
  }
  updateEntityList();
  render(false);
}

function updateEntityList() {
  entityList.replaceChildren();
  const values = state.primitives.filter(visible);
  visibleCount.textContent = String(values.length);
  values.forEach((primitive) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "entity-row" + (primitive === state.selected ? " selected" : "");
    const kind = document.createElement("span");
    kind.className = "entity-kind";
    kind.textContent = primitive.metadata.kind;
    const content = document.createElement("span");
    const key = document.createElement("span");
    key.className = "entity-key";
    key.textContent = primitive.metadata.target_key;
    const source = document.createElement("span");
    source.className = "entity-key";
    source.textContent = sourceSummary(primitive.metadata);
    content.append(key, document.createElement("br"), source);
    button.append(kind, content);
    button.addEventListener("click", () => selectPrimitive(primitive));
    entityList.append(button);
  });
}

function appendOptions(select, values, formatter = (value) => value) {
  [...new Set(values)].sort().forEach((value) => {
    const option = document.createElement("option");
    option.value = String(value);
    option.textContent = formatter(value);
    select.append(option);
  });
}

function updateStatus() {
  const source = state.manifest.source.complete;
  const conversion = state.manifest.conversion.complete;
  const occt = state.manifest.conversion.occt_valid;
  const statuses = [
    ["source-status", `Source ${source ? "complete" : "incomplete"}`, source],
    ["conversion-status", `Conversion ${conversion ? "complete" : "incomplete"}`, conversion],
    ["occt-status", `OCCT ${occt ? "valid" : "invalid"}`, occt],
  ];
  statuses.forEach(([id, text, good]) => {
    const element = document.getElementById(id);
    element.textContent = text;
    element.classList.add(good ? "good" : "bad");
  });
  document.getElementById("bundle-version").textContent = state.manifest.asset_bundle.version;
  if (state.manifest.preview.partial) {
    const banner = document.getElementById("partial-banner");
    banner.hidden = false;
    const missingList = document.getElementById("missing-list");
    state.manifest.missing_entities.forEach((item) => {
      const row = document.createElement("li");
      row.textContent = `${item.source_key}: ${item.reason}`;
      missingList.append(row);
    });
    if (!state.manifest.missing_entities.length) {
      const row = document.createElement("li");
      row.textContent = "The parser marked the source model incomplete.";
      missingList.append(row);
    }
  }
}

function fitView() {
  const bounds = state.manifest.preview.bounds;
  state.center = [
    (bounds[0] + bounds[3]) / 2,
    (bounds[1] + bounds[4]) / 2,
    (bounds[2] + bounds[5]) / 2,
  ];
  state.radius = Math.max(
    Math.hypot(bounds[3] - bounds[0], bounds[4] - bounds[1], bounds[5] - bounds[2]) / 2,
    0.001,
  );
  state.distance = state.radius * 2.8;
  state.yaw = -0.7;
  state.pitch = 0.48;
  render(false);
}

function findPickPixel(predicate = () => true) {
  const gl = state.gl;
  render(true);
  gl.bindFramebuffer(gl.FRAMEBUFFER, state.pickFramebuffer);
  const pixels = new Uint8Array(canvas.width * canvas.height * 4);
  gl.readPixels(0, 0, canvas.width, canvas.height, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
  gl.bindFramebuffer(gl.FRAMEBUFFER, null);
  const centerX = Math.floor(canvas.width / 2);
  const centerY = Math.floor(canvas.height / 2);
  const maximum = Math.max(canvas.width, canvas.height);
  for (let radius = 0; radius < maximum; radius += 2) {
    for (let offset = -radius; offset <= radius; offset += 2) {
      const candidates = [
        [centerX + offset, centerY - radius],
        [centerX + offset, centerY + radius],
        [centerX - radius, centerY + offset],
        [centerX + radius, centerY + offset],
      ];
      for (const [x, y] of candidates) {
        if (x < 0 || y < 0 || x >= canvas.width || y >= canvas.height) {
          continue;
        }
        const index = (y * canvas.width + x) * 4;
        const id = pixels[index] + (pixels[index + 1] << 8) + (pixels[index + 2] << 16);
        const primitive = state.primitives.find((item) => item.metadata.pick_id === id);
        if (primitive && predicate(primitive.metadata)) {
          return [x, y];
        }
      }
    }
  }
  return null;
}

function dispatchCanvasClick(pixel) {
  const rectangle = canvas.getBoundingClientRect();
  const clientX = rectangle.left + ((pixel[0] + 0.5) / canvas.width) * rectangle.width;
  const clientY = rectangle.bottom - ((pixel[1] + 0.5) / canvas.height) * rectangle.height;
  canvas.dispatchEvent(new MouseEvent("click", { clientX, clientY, bubbles: true }));
}

function runSelfTest() {
  selfTestOutput.dataset.status = "running";
  selfTestOutput.textContent = "running";
  pickKind.value = "face";
  bodyFilter.value = "";
  surfaceFilter.value = "";
  diagnosticFilter.value = "";
  const facePixel = findPickPixel((metadata) => metadata.parasolid_face_ids.length > 0);
  if (!facePixel) {
    throw new Error("self-test could not find a rendered face pixel");
  }
  dispatchCanvasClick(facePixel);
  const faceMetadata = state.selected && state.selected.metadata;
  const faceSource = faceMetadata && faceMetadata.source_entities.find(
    (item) => item.kind === "face" && item.byte_range,
  );
  if (
    !faceMetadata
    || faceMetadata.kind !== "face"
    || !faceMetadata.parasolid_face_ids.length
    || !faceSource
  ) {
    throw new Error("self-test face selection did not resolve Parasolid provenance");
  }
  pickKind.value = "edge";
  const edgePixel = findPickPixel((metadata) => metadata.parasolid_edge_ids.length > 0);
  if (!edgePixel) {
    throw new Error("self-test could not find a rendered source edge pixel");
  }
  dispatchCanvasClick(edgePixel);
  const edgeMetadata = state.selected && state.selected.metadata;
  const edgeSource = edgeMetadata && edgeMetadata.source_entities.find(
    (item) => item.kind === "edge" && item.byte_range,
  );
  if (
    !edgeMetadata
    || edgeMetadata.kind !== "edge"
    || !edgeMetadata.parasolid_edge_ids.length
    || !edgeSource
  ) {
    throw new Error("self-test edge selection did not resolve Parasolid provenance");
  }
  selfTestOutput.dataset.status = "passed";
  selfTestOutput.textContent = [
    "passed",
    faceMetadata.target_key,
    faceSource.byte_range.start,
    edgeMetadata.target_key,
    edgeSource.byte_range.start,
  ].join(":");
}

async function initialize() {
  const gl = canvas.getContext("webgl", { antialias: true, alpha: false });
  if (!gl || !gl.getExtension("OES_element_index_uint")) {
    throw new Error("WebGL with 32-bit element indices is required");
  }
  state.gl = gl;
  state.mainProgram = compileProgram(gl, vertexShader, fragmentShader);
  state.pickProgram = compileProgram(gl, pickVertexShader, pickFragmentShader);
  const [manifestResponse, glbResponse] = await Promise.all([
    fetch("preview.manifest.json", { cache: "no-store" }),
    fetch("preview.glb", { cache: "no-store" }),
  ]);
  if (!manifestResponse.ok || !glbResponse.ok) {
    throw new Error("preview artifacts could not be loaded");
  }
  state.manifest = await manifestResponse.json();
  const glb = parseGlb(await glbResponse.arrayBuffer());
  const primitives = glb.document.meshes[0].primitives;
  if (primitives.length !== state.manifest.primitives.length) {
    throw new Error("GLB primitive count differs from the source manifest");
  }
  state.primitives = primitives.map((primitive, index) => {
    const metadata = state.manifest.primitives[index];
    if (metadata.primitive_index !== index || primitive.extras.pickId !== metadata.pick_id) {
      throw new Error("GLB primitive order differs from the source manifest");
    }
    return uploadPrimitive(gl, glb, primitive, metadata);
  });
  appendOptions(bodyFilter, state.primitives.flatMap((item) => item.metadata.body_ids), String);
  appendOptions(
    surfaceFilter,
    state.primitives.flatMap((item) => item.metadata.surface_kinds),
  );
  appendOptions(
    diagnosticFilter,
    state.primitives.flatMap((item) => item.metadata.diagnostic_codes),
  );
  updateStatus();
  updateEntityList();
  fitView();
  document.body.dataset.viewerReady = "true";
  window.__parasolidKitViewer = {
    manifest: state.manifest,
    fit: fitView,
    pickAt,
    selectPickId: (id) => selectPrimitive(
      state.primitives.find((item) => item.metadata.pick_id === id) || null,
    ),
    selected: () => state.selected && state.selected.metadata,
  };
  if (new URLSearchParams(window.location.search).get("self-test") === "1") {
    window.setTimeout(() => {
      try {
        runSelfTest();
      } catch (error) {
        selfTestOutput.dataset.status = "failed";
        selfTestOutput.textContent = `failed:${error.message}`;
      }
    }, 80);
  }
}

canvas.addEventListener("pointerdown", (event) => {
  state.dragging = true;
  state.moved = false;
  state.pointer = [event.clientX, event.clientY];
  canvas.setPointerCapture(event.pointerId);
});

canvas.addEventListener("pointermove", (event) => {
  if (!state.dragging) {
    return;
  }
  const dx = event.clientX - state.pointer[0];
  const dy = event.clientY - state.pointer[1];
  if (Math.abs(dx) + Math.abs(dy) > 1) {
    state.moved = true;
  }
  state.yaw -= dx * 0.008;
  state.pitch = Math.max(-1.45, Math.min(1.45, state.pitch + dy * 0.008));
  state.pointer = [event.clientX, event.clientY];
  render(false);
});

canvas.addEventListener("pointerup", () => {
  state.dragging = false;
});

canvas.addEventListener("click", (event) => {
  if (state.moved) {
    state.moved = false;
    return;
  }
  const rectangle = canvas.getBoundingClientRect();
  const x = Math.floor(((event.clientX - rectangle.left) / rectangle.width) * canvas.width);
  const y = Math.floor(((rectangle.bottom - event.clientY) / rectangle.height) * canvas.height);
  pickAt(x, y);
});

canvas.addEventListener("wheel", (event) => {
  event.preventDefault();
  state.distance = Math.max(
    state.radius * 0.08,
    Math.min(state.radius * 30, state.distance * Math.exp(event.deltaY * 0.001)),
  );
  render(false);
}, { passive: false });

document.getElementById("fit-view").addEventListener("click", fitView);
[pickKind, bodyFilter, surfaceFilter, diagnosticFilter].forEach((element) => {
  element.addEventListener("change", () => {
    if (state.selected && !visible(state.selected)) {
      state.selected = null;
    }
    updateEntityList();
    render(false);
  });
});

window.addEventListener("resize", () => render(false));

initialize().catch((error) => {
  document.body.dataset.viewerReady = "false";
  selectionTitle.textContent = "Preview failed";
  selectionDetails.replaceChildren();
  addDetail("Error", error.message);
  selfTestOutput.dataset.status = "failed";
  selfTestOutput.textContent = `failed:${error.message}`;
});
