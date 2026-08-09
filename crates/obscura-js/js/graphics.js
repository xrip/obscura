// Chrome 145 Windows graphics facade. This file is inserted inside the
// bootstrap closure after _Canvas2D so it can use the private DOM helpers.
const _GRAPHICS_PIXEL_WORK_LIMIT = 64 * 1024 * 1024;
const _WEBGL_SHADOW_LIMIT = 8 * 1024 * 1024;
const _GRAPHICS_COMMAND_LIMIT = 1024;
const _GRAPHICS_ERROR_LIMIT = 32;
const _graphicsObjectToken = {};
const _canvasSlots = new WeakMap();
const _webglSlots = new WeakMap();
const _resourceSlots = new WeakMap();

function _graphicsIllegalConstructor() { throw new TypeError('Illegal constructor'); }
function _graphicsBrand(slots, self, name) {
  const state = slots.get(self);
  if (!state) throw new TypeError("Illegal invocation");
  return state;
}
function _graphicsUint(value, fallback) {
  const n = Number(value);
  if (!Number.isFinite(n) || n < 0) return fallback;
  return Math.min(0x7fffffff, Math.floor(n));
}
function _canvasAttributeSize(value, fallback) {
  if (value === null || value === undefined || value === '') return fallback;
  return _graphicsUint(value, fallback);
}
function _graphicsSetFunctionShape(fn, name, length) {
  return _makeNativeFunction(fn, name, length);
}
function _graphicsDefineMethod(proto, name, length, fn) {
  Object.defineProperty(proto, name, {value:_graphicsSetFunctionShape(fn, name, length), writable:true, enumerable:true, configurable:true});
}
function _graphicsDefineProperties(proto, descriptors) {
  for (const name of Object.keys(descriptors)) {
    const d = descriptors[name];
    if (typeof d.get === 'function') d.get = _makeNativeFunction(d.get, 'get ' + name, 0, 'function get ' + name + '() { [native code] }');
    if (typeof d.set === 'function') d.set = _makeNativeFunction(d.set, 'set ' + name, 1, 'function set ' + name + '() { [native code] }');
  }
  Object.defineProperties(proto, descriptors);
}
function _graphicsTag(proto, name) {
  Object.defineProperty(proto, Symbol.toStringTag, {value:name, configurable:true});
}
function _graphicsDefineConstants(C, constants) {
  for (const name of Object.keys(constants)) {
    const d = {value:constants[name], writable:false, enumerable:true, configurable:false};
    if (!Object.prototype.hasOwnProperty.call(C, name)) Object.defineProperty(C, name, d);
    if (!Object.prototype.hasOwnProperty.call(C.prototype, name)) Object.defineProperty(C.prototype, name, d);
  }
}
function _graphicsBytes(data) {
  if (data == null) return null;
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  return null;
}
function _graphicsHashText(text, seed) {
  let a = (0x811c9dc5 ^ (seed || 0)) >>> 0;
  let b = (0x9e3779b9 ^ ((seed || 0) * 0x85ebca6b)) >>> 0;
  text = String(text);
  for (let i = 0; i < text.length; i++) {
    const c = text.charCodeAt(i);
    a = Math.imul(a ^ c, 0x01000193) >>> 0;
    b = Math.imul(b ^ (c + i), 0x27d4eb2d) >>> 0;
    b = ((b << 13) | (b >>> 19)) >>> 0;
  }
  return [a, b];
}
function _graphicsHashBytes(bytes, seed) {
  if (!bytes) return _graphicsHashText('null', seed);
  const limit = 1024 * 1024;
  let a = (0x811c9dc5 ^ (seed || 0) ^ bytes.byteLength) >>> 0;
  let b = (0x9e3779b9 ^ bytes.byteLength) >>> 0;
  const count = Math.min(bytes.byteLength, limit);
  for (let i = 0; i < count; i++) {
    const at = bytes.byteLength <= limit ? i : Math.floor(i * (bytes.byteLength - 1) / (limit - 1));
    const c = bytes[at];
    a = Math.imul(a ^ c, 0x01000193) >>> 0;
    b = Math.imul(b ^ (c + at), 0x27d4eb2d) >>> 0;
  }
  return [a, b];
}
function _graphicsDigest(value) {
  let text;
  try { text = JSON.stringify(value, function(k, v) {
    if (ArrayBuffer.isView(v)) return Array.from(v);
    if (v instanceof ArrayBuffer) return Array.from(new Uint8Array(v));
    if (_resourceSlots.has(v)) { const s = _resourceSlots.get(v); return [s.kind, s.serial, s.digest]; }
    return v;
  }); } catch (_) { text = String(value); }
  const h = _graphicsHashText(text || '');
  return h[0].toString(16).padStart(8,'0') + h[1].toString(16).padStart(8,'0');
}

function _newSurface(width, height) {
  return {width, height, base:[0,0,0,0], regions:[], serial:0};
}
function _surfaceReset(surface, width, height) {
  surface.width = width; surface.height = height; surface.base = [0,0,0,0];
  surface.regions.length = 0; surface.serial++;
}
function _surfaceRegion(surface, region) {
  if (region.x <= 0 && region.y <= 0 && region.w >= surface.width && region.h >= surface.height && region.kind === 'clear') {
    surface.base = region.color.slice(); surface.regions.length = 0; surface.serial++; return;
  }
  surface.regions.push(region);
  if (surface.regions.length > 256) {
    const old = surface.regions.splice(0, surface.regions.length - 128);
    const digest = _graphicsDigest(old);
    surface.regions.unshift({kind:'draw',x:0,y:0,w:surface.width,h:surface.height,hash:_graphicsHashText(digest),mask:[true,true,true,true]});
  }
  surface.serial++;
}
function _surfacePixel(surface, x, y) {
  let out = surface.base.slice();
  for (let i = 0; i < surface.regions.length; i++) {
    const r = surface.regions[i];
    if (x < r.x || y < r.y || x >= r.x + r.w || y >= r.y + r.h) continue;
    let c;
    if (r.kind === 'draw') {
      const h = r.hash || [0,0];
      let v = Math.imul((x + 1) ^ h[0], 0x45d9f3b) ^ Math.imul((y + 1) ^ h[1], 0x119de1f3);
      v ^= v >>> 16;
      c = [v & 255, (v >>> 8) & 255, (v >>> 16) & 255, 255];
    } else if (r.kind === 'pixels') {
      const px=x-r.x,py=y-r.y,at=py*r.bytesPerRow+px*4;c=[r.bytes[at]||0,r.bytes[at+1]||0,r.bytes[at+2]||0,r.bytes[at+3]??255];if(r.bgra)c=[c[2],c[1],c[0],c[3]];
    } else c = r.color;
    const m = r.mask || [true,true,true,true];
    for (let k = 0; k < 4; k++) if (m[k]) out[k] = c[k];
  }
  return out;
}
function _surfaceMaterialize(surface, topLeft) {
  const pixels = surface.width * surface.height;
  if (!Number.isSafeInteger(pixels) || pixels * 4 > _GRAPHICS_PIXEL_WORK_LIMIT) return null;
  const out = new Uint8ClampedArray(pixels * 4);
  for (let row = 0; row < surface.height; row++) {
    const y = topLeft ? surface.height - 1 - row : row;
    for (let x = 0; x < surface.width; x++) out.set(_surfacePixel(surface, x, y), (row * surface.width + x) * 4);
  }
  return out;
}

function _canvasSize(canvas) {
  const s = _canvasSlots.get(canvas);
  return s ? [s.width, s.height] : [300, 150];
}
function _canvasState(canvas, width, height) {
  let s = _canvasSlots.get(canvas);
  if (!s) {
    s = {mode:null, context:null, width, height, generation:0, surface:_newSurface(width, height)};
    _canvasSlots.set(canvas, s);
  }
  return s;
}
function _resetCanvas(canvas, width, height) {
  const s = _canvasState(canvas, width, height);
  s.width = width; s.height = height; s.generation++;
  _surfaceReset(s.surface, width, height);
  if (s.mode === '2d' && s.context) {
    s.context._w = width; s.context._h = height;
    const bytes = width * height * 4;
    s.context._buf = Number.isSafeInteger(bytes) && bytes <= _GRAPHICS_PIXEL_WORK_LIMIT ? new Uint8ClampedArray(bytes) : new Uint8ClampedArray(0);
  }
  if ((s.mode === 'webgl' || s.mode === 'webgl2') && s.context) _webglResize(s.context);
  if (s.mode === 'webgpu' && s.context) _gpuCanvasResize(s.context);
}
function _canvasPng(canvas) {
  const s = _canvasSlots.get(canvas);
  if (!s || !s.width || !s.height) return 'data:,';
  if (s.mode === '2d' && s.context && s.context._buf.length === s.width * s.height * 4) return _encodePNG(s.width, s.height, s.context._buf);
  const bytes = _surfaceMaterialize(s.surface, true);
  return bytes ? _encodePNG(s.width, s.height, bytes) : 'data:,';
}
function _pngBlob(canvas) {
  const url = _canvasPng(canvas);
  if (url === 'data:,') return new Blob([], {type:'image/png'});
  return new Blob([_base64ToUint8Array(url.slice(url.indexOf(',') + 1))], {type:'image/png'});
}

// Named class *expression*: upstream's bootstrap already declares a top-level
// `class HTMLCanvasElement`, and two lexical declarations of one name in the
// same scope is a SyntaxError. A class expression keeps its own name binding
// inside the class body only, so there is no clash, while `.name` still reports
// "HTMLCanvasElement" as Chrome does.
const _ObscuraHTMLCanvasElement = class HTMLCanvasElement extends Element {
  // Fork note: upstream's bootstrap constructs every element as `new C(nid)`
  // from three call sites, and its own element classes take no construction
  // token. Taking a plain nid here keeps those three sites untouched. Upstream
  // does not guard its element constructors either, so this matches it.
  constructor(nid) {
    // Chrome throws for `new HTMLCanvasElement()`. Upstream's element factory
    // always passes the numeric node id, so the argument type separates an
    // engine construction from a page one without needing a private token.
    if (typeof nid !== 'number') {
      throw new TypeError("Failed to construct 'HTMLCanvasElement': Illegal constructor");
    }
    super(nid);
    const w = _canvasAttributeSize(super.getAttribute('width'), 300);
    const h = _canvasAttributeSize(super.getAttribute('height'), 150);
    _canvasState(this, w, h);
  }
  get width() { return _canvasSlots.get(this).width; }
  set width(v) { const n = _graphicsUint(v, 0); super.setAttribute('width', String(n)); _resetCanvas(this, n, this.height); }
  get height() { return _canvasSlots.get(this).height; }
  set height(v) { const n = _graphicsUint(v, 0); super.setAttribute('height', String(n)); _resetCanvas(this, this.width, n); }
  setAttribute(name, value) {
    super.setAttribute(name, value);
    const n = String(name).toLowerCase();
    if (n === 'width' || n === 'height') {
      const w = _canvasAttributeSize(super.getAttribute('width'), 300);
      const h = _canvasAttributeSize(super.getAttribute('height'), 150);
      _resetCanvas(this, w, h);
    }
  }
  removeAttribute(name) {
    super.removeAttribute(name);
    const n = String(name).toLowerCase();
    if (n === 'width' || n === 'height') _resetCanvas(this, _canvasAttributeSize(super.getAttribute('width'), 300), _canvasAttributeSize(super.getAttribute('height'), 150));
  }
  getContext(type, options) { return _canvasGetContext(this, type, options); }
  toDataURL(type, quality) { return !type || String(type).toLowerCase() === 'image/png' ? _canvasPng(this) : _canvasPng(this); }
  toBlob(callback, type, quality) {
    if (typeof callback !== 'function') throw new TypeError("Failed to execute 'toBlob': parameter 1 is not of type 'Function'.");
    const blob = _pngBlob(this); setTimeout(function(){ callback(blob); }, 0);
  }
  transferControlToOffscreen() {
    const out = new OffscreenCanvas(this.width, this.height);
    const src = _canvasSlots.get(this), dst = _canvasSlots.get(out);
    dst.surface = src.surface; return out;
  }
};
_graphicsTag(_ObscuraHTMLCanvasElement.prototype, 'HTMLCanvasElement');
// Replaces upstream's HTMLCanvasElement. _elementClassFor already resolves
// "CANVAS" through globalThis.HTMLCanvasElement, so every canvas built after
// this point is ours without touching the element factory.
_graphicsDefineGlobal('HTMLCanvasElement', _ObscuraHTMLCanvasElement);

class OffscreenCanvas {
  constructor(width, height) {
    width = _graphicsUint(width, 0); height = _graphicsUint(height, 0);
    _canvasState(this, width, height);
  }
  get width() { return _graphicsBrand(_canvasSlots, this, 'OffscreenCanvas').width; }
  set width(v) { _resetCanvas(this, _graphicsUint(v, 0), this.height); }
  get height() { return _graphicsBrand(_canvasSlots, this, 'OffscreenCanvas').height; }
  set height(v) { _resetCanvas(this, this.width, _graphicsUint(v, 0)); }
  getAttribute(name) { return String(name).toLowerCase() === 'width' ? String(this.width) : String(name).toLowerCase() === 'height' ? String(this.height) : null; }
  getContext(type, options) { return _canvasGetContext(this, type, options); }
  convertToBlob(options) { return Promise.resolve(_pngBlob(this)); }
  transferToImageBitmap() {
    const s = _canvasSlots.get(this); return new ImageBitmap(_graphicsObjectToken, s.width, s.height, _surfaceMaterialize(s.surface, true));
  }
}
_graphicsTag(OffscreenCanvas.prototype, 'OffscreenCanvas');
_graphicsDefineGlobal('OffscreenCanvas', OffscreenCanvas);

function CanvasRenderingContext2D() { _graphicsIllegalConstructor(); }
CanvasRenderingContext2D.prototype = _Canvas2D.prototype;
Object.defineProperty(CanvasRenderingContext2D.prototype, 'constructor', {value:CanvasRenderingContext2D, writable:true, configurable:true});
_graphicsTag(CanvasRenderingContext2D.prototype, 'CanvasRenderingContext2D');
_graphicsDefineGlobal('CanvasRenderingContext2D', CanvasRenderingContext2D);

class ImageBitmap {
  constructor(token, width, height, bytes) {
    if (token !== _graphicsObjectToken) _graphicsIllegalConstructor();
    this.width = width; this.height = height; this._graphicsBytes = bytes; this._closed = false;
  }
  close() { this._closed = true; this.width = 0; this.height = 0; this._graphicsBytes = null; }
}
_graphicsTag(ImageBitmap.prototype, 'ImageBitmap');
_graphicsDefineGlobal('ImageBitmap', ImageBitmap);
// A Window *method*, not an interface object: Chrome lists it in
// Object.keys(window), so it stays a plain enumerable assignment.
globalThis.createImageBitmap = function createImageBitmap(source) {
  const s = source && _canvasSlots.get(source);
  return Promise.resolve(new ImageBitmap(_graphicsObjectToken, s ? s.width : 0, s ? s.height : 0, s ? _surfaceMaterialize(s.surface, true) : null));
};

for (const pair of [[_ObscuraHTMLCanvasElement,'HTMLCanvasElement'],[OffscreenCanvas,'OffscreenCanvas'],[CanvasRenderingContext2D,'CanvasRenderingContext2D'],[ImageBitmap,'ImageBitmap']]) _markNative(pair[0]);
for (const pair of [[_ObscuraHTMLCanvasElement.prototype,'getContext',2],[_ObscuraHTMLCanvasElement.prototype,'toDataURL',0],[_ObscuraHTMLCanvasElement.prototype,'toBlob',1],[_ObscuraHTMLCanvasElement.prototype,'transferControlToOffscreen',0],[OffscreenCanvas.prototype,'getContext',2],[OffscreenCanvas.prototype,'convertToBlob',0],[OffscreenCanvas.prototype,'transferToImageBitmap',0],[ImageBitmap.prototype,'close',0]]) {
  const fn = pair[0][pair[1]]; if (typeof fn === 'function') _graphicsDefineMethod(pair[0], pair[1], pair[2], fn);
}
for (const pair of [[_ObscuraHTMLCanvasElement.prototype,'width'],[_ObscuraHTMLCanvasElement.prototype,'height'],[OffscreenCanvas.prototype,'width'],[OffscreenCanvas.prototype,'height']]) {
  const d=Object.getOwnPropertyDescriptor(pair[0],pair[1]);if(d)Object.defineProperty(pair[0],pair[1],Object.assign({},d,{enumerable:true}));
}
for(const name of Object.getOwnPropertyNames(_Canvas2D.prototype)){if(name==='constructor')continue;const d=Object.getOwnPropertyDescriptor(_Canvas2D.prototype,name);if(d&&typeof d.value==='function')_graphicsDefineMethod(_Canvas2D.prototype,name,d.value.length,d.value);}

function _webglComponent(version) {
  const graphics = _fingerprintProfile && _fingerprintProfile.graphics;
  return graphics && graphics[version === 2 ? 'webgl2' : 'webgl1'] || {contextAttributes:{alpha:true,antialias:true,depth:true,stencil:false,premultipliedAlpha:true,preserveDrawingBuffer:false,powerPreference:'default',failIfMajorPerformanceCaveat:false,desynchronized:false,xrCompatible:false},parameters:{},initialState:{},extensions:{},supportedExtensions:[],shaderPrecisionFormats:[]};
}
function _webglValue(entry) {
  if (!entry) return null;
  const value = entry.value;
  if (Array.isArray(value)) {
    if (entry.type === 'Float32Array') return new Float32Array(value);
    if (entry.type === 'Int32Array') return new Int32Array(value);
    if (entry.type === 'Uint32Array') return new Uint32Array(value);
    return value.slice();
  }
  return value;
}
function _webglPushError(s, error) {
  if (!error || s.errors.indexOf(error) >= 0 || s.errors.length >= _GRAPHICS_ERROR_LIMIT) return;
  s.errors.push(error);
}
function _webglState(self) { return _graphicsBrand(_webglSlots, self, self && self.constructor && self.constructor.name || 'WebGLRenderingContext'); }
function _webglLive(s) { return !s.lost; }
function _webglResource(s, value, kind, allowNull) {
  if (value === null && allowNull) return null;
  const r = _resourceSlots.get(value);
  if (!r || r.kind !== kind || r.context !== s || r.deleted || r.generation !== s.generation) {
    _webglPushError(s, 0x0502); return undefined;
  }
  return r;
}
let _webglResourceSerial = 0;
function _newWebglResource(C, s, kind, extra) {
  const object = new C(_graphicsObjectToken);
  _resourceSlots.set(object, Object.assign({kind,context:s,deleted:false,generation:s.generation,serial:++_webglResourceSerial,digest:'0'}, extra || {}));
  return object;
}
function _webglResourceClass(name) {
  const C = class { constructor(token) { if (token !== _graphicsObjectToken) _graphicsIllegalConstructor(); } };
  Object.defineProperty(C, 'name', {value:name}); _graphicsTag(C.prototype, name); _markNative(C); _graphicsDefineGlobal(name, C); return C;
}
const WebGLBuffer = _webglResourceClass('WebGLBuffer');
const WebGLTexture = _webglResourceClass('WebGLTexture');
const WebGLFramebuffer = _webglResourceClass('WebGLFramebuffer');
const WebGLRenderbuffer = _webglResourceClass('WebGLRenderbuffer');
const WebGLShader = _webglResourceClass('WebGLShader');
const WebGLProgram = _webglResourceClass('WebGLProgram');
const WebGLUniformLocation = _webglResourceClass('WebGLUniformLocation');
const WebGLVertexArrayObject = _webglResourceClass('WebGLVertexArrayObject');
const WebGLQuery = _webglResourceClass('WebGLQuery');
const WebGLSampler = _webglResourceClass('WebGLSampler');
const WebGLSync = _webglResourceClass('WebGLSync');
const WebGLTransformFeedback = _webglResourceClass('WebGLTransformFeedback');
class WebGLActiveInfo {
  constructor(token, size, type, name) { if (token !== _graphicsObjectToken) _graphicsIllegalConstructor(); this.size=size;this.type=type;this.name=name; }
}
class WebGLShaderPrecisionFormat {
  constructor(token, min, max, precision) { if (token !== _graphicsObjectToken) _graphicsIllegalConstructor(); this.rangeMin=min;this.rangeMax=max;this.precision=precision; }
}
for (const C of [WebGLActiveInfo,WebGLShaderPrecisionFormat]) { _graphicsTag(C.prototype,C.name);_markNative(C);_graphicsDefineGlobal(C.name,C); }

class WebGLRenderingContext { constructor(token, canvas, attrs) { if (token !== _graphicsObjectToken) _graphicsIllegalConstructor(); _initWebgl(this, canvas, attrs, 1); } }
class WebGL2RenderingContext { constructor(token, canvas, attrs) { if (token !== _graphicsObjectToken) _graphicsIllegalConstructor(); _initWebgl(this, canvas, attrs, 2); } }
for (const C of [WebGLRenderingContext,WebGL2RenderingContext]) { _graphicsTag(C.prototype,C.name);_markNative(C);_graphicsDefineGlobal(C.name,C); }
_graphicsDefineConstants(WebGLRenderingContext, _WEBGL1_CONSTANTS);
_graphicsDefineConstants(WebGL2RenderingContext, _WEBGL2_CONSTANTS);

function _initWebgl(self, canvas, options, version) {
  const component = _webglComponent(version), defaults = component.contextAttributes || {};
  const attrs = {};
  for (const key of Object.keys(defaults)) attrs[key] = options && Object.prototype.hasOwnProperty.call(options,key) ? (key === 'powerPreference' ? String(options[key]) : !!options[key]) : defaults[key];
  const cs = _canvasSlots.get(canvas), width = Math.max(1, cs.width), height = Math.max(1, cs.height);
  const dynamic = new Map();
  for (const key of Object.keys(component.initialState || {})) dynamic.set(+key, _webglValue(component.initialState[key]));
  dynamic.set(0x0ba2, new Int32Array([0,0,width,height]));
  dynamic.set(0x0c10, new Int32Array([0,0,width,height]));
  if (cs.surface.width !== width || cs.surface.height !== height) _surfaceReset(cs.surface,width,height);
  const s = {self,canvas,version,component,attrs,dynamic,errors:[],lost:false,generation:1,resources:new Set(),bindings:new Map(),textureUnits:[],activeTexture:0,enabled:new Set(),extensions:new Map(),extensionNames:new Set(),currentProgram:null,shadowBytes:0,drawNumber:0,commands:[],uniformValues:new Map(),vertexAttribs:new Map(),defaultSurface:cs.surface,readFramebuffer:null,drawFramebuffer:null,renderbuffer:null,vao:null,query:null,drawingBufferColorSpace:'srgb',unpackColorSpace:'srgb'};
  _webglSlots.set(self,s);
}
function _webglReset(self) {
  const s = _webglSlots.get(self); if (!s) return;
  s.generation++; s.errors.length=0;s.lost=false;s.bindings.clear();s.textureUnits.length=0;s.enabled.clear();s.currentProgram=null;s.shadowBytes=0;s.drawNumber=0;s.commands.length=0;s.uniformValues.clear();s.vertexAttribs.clear();s.readFramebuffer=null;s.drawFramebuffer=null;s.renderbuffer=null;s.vao=null;
  const size = _canvasSize(s.canvas), w=Math.max(1,size[0]), h=Math.max(1,size[1]);
  _surfaceReset(s.defaultSurface,w,h);
  s.dynamic = new Map(); for (const key of Object.keys(s.component.initialState || {})) s.dynamic.set(+key,_webglValue(s.component.initialState[key]));
  s.dynamic.set(0x0ba2,new Int32Array([0,0,w,h]));s.dynamic.set(0x0c10,new Int32Array([0,0,w,h]));
}
function _webglResize(self) {
  const s = _webglSlots.get(self); if (!s) return;
  const size = _canvasSize(s.canvas), w=Math.max(1,size[0]), h=Math.max(1,size[1]);
  _surfaceReset(s.defaultSurface,w,h);
}
function _webglSurface(s, read) {
  const fb = read ? s.readFramebuffer : s.drawFramebuffer;
  if (fb) {
    const fr = _resourceSlots.get(fb), attachment = fr && fr.attachments && fr.attachments.get(0x8ce0);
    const ar = attachment && _resourceSlots.get(attachment.object);
    if (ar && ar.surface) return ar.surface;
  }
  return s.defaultSurface;
}
function _webglGetParameter(pname) {
  const s=_webglState(this); pname=Number(pname);
  if (!_webglLive(s)) return null;
  if (pname===0x9245 || pname===0x9246) {
    if (!s.extensionNames.has('webgl_debug_renderer_info')) { _webglPushError(s,0x0500);return null; }
    const g=_fingerprintProfile&&_fingerprintProfile.graphics||{};return pname===0x9245?g.unmaskedVendor:g.unmaskedRenderer;
  }
  const bindings={0x8894:0x8892,0x8895:0x8893,0x8ca6:'drawFramebuffer',0x8caa:'readFramebuffer',0x8ca7:'renderbuffer',0x8b8d:'currentProgram',0x85b5:'vao'};
  if (Object.prototype.hasOwnProperty.call(bindings,pname)) { const k=bindings[pname];return typeof k==='number'?(s.bindings.get(k)||null):(s[k]||null); }
  if (pname===0x8069 || pname===0x8514 || pname===0x806a || pname===0x8c1d) { const unit=s.textureUnits[s.activeTexture]||{};return unit[pname]||null; }
  if (pname===0x9245 || pname===0x9246) return null;
  if (s.dynamic.has(pname)) { const v=s.dynamic.get(pname);return ArrayBuffer.isView(v)?new v.constructor(v):Array.isArray(v)?v.slice():v; }
  const entry=s.component.parameters&&s.component.parameters[String(pname)];if(entry)return _webglValue(entry);
  const ext=s.component.extensions&&s.component.extensions[String(pname)];
  if(ext&&s.extensionNames.has(String(ext.name).toLowerCase())) {
    if(pname===0x84ff)return s.component.maxAnisotropy||1;
    return 0;
  }
  _webglPushError(s,0x0500);return null;
}
function _webglContextAttributes() { const s=_webglState(this);return s.lost?null:Object.assign({},s.attrs); }
function _webglGetError(){const s=_webglState(this);return s.errors.length?s.errors.shift():s.lost?0x9242:0;}
function _webglSupportedExtensions(){const s=_webglState(this);return s.lost?null:(s.component.supportedExtensions||[]).slice();}
function _webglExtension(name) {
  const s=_webglState(this);if(s.lost)return null;const original=String(name),key=original.toLowerCase();
  const supported=(s.component.supportedExtensions||[]).find(n=>n.toLowerCase()===key);if(!supported)return null;
  if(s.extensions.has(key))return s.extensions.get(key);
  const ext={};s.extensionNames.add(key);
  for(const enumKey of Object.keys(s.component.extensions||{})){const e=s.component.extensions[enumKey];if(String(e.name).toLowerCase()===key)Object.defineProperty(ext,e.constantName,{value:+enumKey,enumerable:true});}
  const add=(n,l,f)=>_graphicsDefineMethod(ext,n,l,f);
  if(key==='webgl_lose_context') { add('loseContext',0,function(){_webglLose(s);});add('restoreContext',0,function(){_webglRestore(s);}); }
  if(key==='angle_instanced_arrays'){add('drawArraysInstancedANGLE',4,function(a,b,c,d){_webglDraw.call(s.self,'drawArraysInstancedANGLE',[a,b,c,d]);});add('drawElementsInstancedANGLE',5,function(a,b,c,d,e){_webglDraw.call(s.self,'drawElementsInstancedANGLE',[a,b,c,d,e]);});add('vertexAttribDivisorANGLE',2,function(){});}
  if(key==='oes_vertex_array_object'){add('createVertexArrayOES',0,function(){return _newWebglResource(WebGLVertexArrayObject,s,'vertexArray',{attributes:new Map()});});add('deleteVertexArrayOES',1,function(v){_deleteResource(s,v,'vertexArray');});add('isVertexArrayOES',1,function(v){return !!_webglResource(s,v,'vertexArray',false);});add('bindVertexArrayOES',1,function(v){if(v===null||_webglResource(s,v,'vertexArray',true)!==undefined)s.vao=v;});}
  if(key==='webgl_draw_buffers')add('drawBuffersWEBGL',1,function(values){s.drawBuffers=Array.from(values,Number);});
  if(key==='ext_disjoint_timer_query'||key==='ext_disjoint_timer_query_webgl2'){
    add('createQueryEXT',0,function(){return _newWebglResource(WebGLQuery,s,'query',{active:false,available:false,result:0,target:0});});
    add('deleteQueryEXT',1,function(v){_deleteResource(s,v,'query');});
    add('isQueryEXT',1,function(v){return _isResource(s,v,'query');});
    add('beginQueryEXT',2,function(target,q){const r=_webglResource(s,q,'query',false);if(!r)return;if(s.query){_webglPushError(s,0x0502);return;}r.active=true;r.available=false;r.target=Number(target);s.query=q;});
    add('endQueryEXT',1,function(){if(!s.query){_webglPushError(s,0x0502);return;}const r=_resourceSlots.get(s.query);r.active=false;r.available=true;r.result=Math.max(1,s.drawNumber);s.query=null;});
    add('queryCounterEXT',2,function(q){const r=_webglResource(s,q,'query',false);if(r){r.available=true;r.result=Math.max(1,s.drawNumber);}});
    add('getQueryEXT',2,function(target,pname){return Number(pname)===0x8865?s.query:null;});
    add('getQueryObjectEXT',2,function(q,pname){const r=_webglResource(s,q,'query',false);if(!r)return null;return Number(pname)===0x8867?r.available:r.result;});
  }
  if(key==='webgl_debug_shaders')add('getTranslatedShaderSource',1,function(shader){const r=_webglResource(s,shader,'shader',false);return r?r.source:'';});
  if(key==='webgl_provoking_vertex'){
    if(!Object.prototype.hasOwnProperty.call(ext,'FIRST_VERTEX_CONVENTION_WEBGL'))Object.defineProperty(ext,'FIRST_VERTEX_CONVENTION_WEBGL',{value:0x8e4d,enumerable:true});
    if(!Object.prototype.hasOwnProperty.call(ext,'LAST_VERTEX_CONVENTION_WEBGL'))Object.defineProperty(ext,'LAST_VERTEX_CONVENTION_WEBGL',{value:0x8e4e,enumerable:true});
    if(!Object.prototype.hasOwnProperty.call(ext,'PROVOKING_VERTEX_WEBGL'))Object.defineProperty(ext,'PROVOKING_VERTEX_WEBGL',{value:0x8e4f,enumerable:true});
    add('provokingVertexWEBGL',1,function(mode){mode=Number(mode);if(mode!==0x8e4d&&mode!==0x8e4e){_webglPushError(s,0x0500);return;}s.provokingVertex=mode;});
  }
  if(key==='webgl_multi_draw'){add('multiDrawArraysWEBGL',6,function(){_webglDraw.call(s.self,'multiDrawArraysWEBGL',Array.from(arguments));});add('multiDrawElementsWEBGL',8,function(){_webglDraw.call(s.self,'multiDrawElementsWEBGL',Array.from(arguments));});add('multiDrawArraysInstancedWEBGL',8,function(){_webglDraw.call(s.self,'multiDrawArraysInstancedWEBGL',Array.from(arguments));});add('multiDrawElementsInstancedWEBGL',10,function(){_webglDraw.call(s.self,'multiDrawElementsInstancedWEBGL',Array.from(arguments));});}
  if(key==='khr_parallel_shader_compile'&&!Object.prototype.hasOwnProperty.call(ext,'COMPLETION_STATUS_KHR'))Object.defineProperty(ext,'COMPLETION_STATUS_KHR',{value:0x91b1,enumerable:true});
  s.extensions.set(key,ext);return ext;
}
function _webglLose(s){if(s.lost)return;s.lost=true;try{s.canvas.dispatchEvent&&s.canvas.dispatchEvent(new Event('webglcontextlost',{cancelable:true}));}catch(_){} }
function _webglRestore(s){if(!s.lost)return;_webglReset(s.self);try{s.canvas.dispatchEvent&&s.canvas.dispatchEvent(new Event('webglcontextrestored'));}catch(_){} }
function _webglPrecision(shaderType,precisionType){const s=_webglState(this);const p=(s.component.shaderPrecisionFormats||[]).find(v=>v.shaderType===Number(shaderType)&&v.precisionType===Number(precisionType));if(!p){_webglPushError(s,0x0500);return null;}return new WebGLShaderPrecisionFormat(_graphicsObjectToken,p.rangeMin,p.rangeMax,p.precision);}

function _createResourceMethod(C,kind,extra){return function(){const s=_webglState(this);return s.lost?null:_newWebglResource(C,s,kind,typeof extra==='function'?extra():extra);};}
function _deleteResource(s,value,kind){if(value===null)return;const r=_webglResource(s,value,kind,false);if(r){r.deleted=true;if(r.bytes){s.shadowBytes-=r.bytes.byteLength;r.bytes=null;}}}
function _isResource(s,value,kind){const r=_resourceSlots.get(value);return !!r&&r.context===s&&r.kind===kind&&!r.deleted&&r.generation===s.generation;}
function _bindBuffer(target,buffer){const s=_webglState(this);target=Number(target);if(![0x8892,0x8893,0x8f36,0x8f37,0x88eb,0x88ec,0x8a11,0x8c8e].includes(target)){_webglPushError(s,0x0500);return;}if(buffer!==null&&_webglResource(s,buffer,'buffer',false)===undefined)return;s.bindings.set(target,buffer);}
function _boundBuffer(s,target){const b=s.bindings.get(Number(target));if(!b){_webglPushError(s,0x0502);return null;}return _resourceSlots.get(b);}
function _bufferData(target,src,usage,srcOffset,length){const s=_webglState(this),r=_boundBuffer(s,target);if(!r)return;let size,bytes=null;if(typeof src==='number'){size=Number(src);if(!Number.isSafeInteger(size)||size<0){_webglPushError(s,0x0501);return;}}else{const all=_graphicsBytes(src);if(!all){_webglPushError(s,0x0501);return;}const off=Math.max(0,Number(srcOffset)||0);const count=length===undefined?all.byteLength-off:Number(length);bytes=all.slice(off,off+count);size=bytes.byteLength;}if(size>Number.MAX_SAFE_INTEGER){_webglPushError(s,0x0505);return;}const old=r.bytes?r.bytes.byteLength:0;if(bytes&&s.shadowBytes-old+bytes.byteLength>_WEBGL_SHADOW_LIMIT){_webglPushError(s,0x0505);return;}s.shadowBytes-=old;r.size=size;r.usage=Number(usage);r.bytes=bytes;r.digest=bytes?_graphicsHashBytes(bytes).join(':'):_graphicsDigest([size,usage]);s.shadowBytes+=bytes?bytes.byteLength:0;}
function _bufferSubData(target,offset,src,srcOffset,length){const s=_webglState(this),r=_boundBuffer(s,target);if(!r)return;offset=Number(offset);const all=_graphicsBytes(src);if(!Number.isSafeInteger(offset)||offset<0||!all){_webglPushError(s,0x0501);return;}const off=Math.max(0,Number(srcOffset)||0),count=length===undefined?all.byteLength-off:Number(length),part=all.slice(off,off+count);if(offset+part.byteLength>r.size){_webglPushError(s,0x0501);return;}if(!r.bytes){if(r.size>_WEBGL_SHADOW_LIMIT||s.shadowBytes+r.size>_WEBGL_SHADOW_LIMIT){r.digest=_graphicsDigest([r.digest,offset,_graphicsHashBytes(part)]);return;}r.bytes=new Uint8Array(r.size);s.shadowBytes+=r.size;}r.bytes.set(part,offset);r.digest=_graphicsHashBytes(r.bytes).join(':');}
function _getBufferParameter(target,pname){const s=_webglState(this),r=_boundBuffer(s,target);if(!r)return null;if(pname===0x8764)return r.size||0;if(pname===0x8765)return r.usage||0;_webglPushError(s,0x0500);return null;}

function _shaderSource(shader,source){const s=_webglState(this),r=_webglResource(s,shader,'shader',false);if(!r)return;r.source=String(source);r.compiled=false;r.digest=_graphicsDigest(r.source);}
function _scanShader(source,version,type){const out={ok:true,log:'',uniforms:[],attributes:[],varyings:[],conditional:false,digest:_graphicsDigest(source)};if(!source.trim()){out.ok=false;out.log='ERROR: 0:1: shader source is empty';return out;}let clean=source.replace(/\/\*[\s\S]*?\*\//g,'').replace(/\/\/.*$/gm,'');out.conditional=/^\s*#\s*(?:if|ifdef|ifndef|elif)\b/m.test(clean);let depth=0;for(const ch of clean){if(ch==='{')depth++;if(ch==='}')depth--;if(depth<0)break;}if(depth!==0||!/\bvoid\s+main\s*\(/.test(clean)){out.ok=false;out.log='ERROR: 0:1: malformed shader or missing main';return out;}if(version===1&&/^\s*#version\s+300\s+es/m.test(clean)){out.ok=false;out.log='ERROR: 0:1: GLSL ES 3.00 is not valid in WebGL 1';return out;}if(version===2&&/^\s*#version/m.test(clean)&&!/^\s*#version\s+300\s+es/m.test(clean)){out.ok=false;out.log='ERROR: 0:1: WebGL 2 requires GLSL ES 3.00';return out;}if(/\b(?:syntax_error|INVALID_SHADER)\b/.test(clean)){out.ok=false;out.log='ERROR: 0:1: shader compilation failed';return out;}const re=/\b(uniform|attribute|in|out|varying)\s+(?:lowp\s+|mediump\s+|highp\s+)?(\w+)\s+(\w+)(?:\s*\[\s*(\d+)\s*\])?\s*;/g;let m;while((m=re.exec(clean))){const d={kind:m[1],type:m[2],name:m[3],size:+m[4]||1};if(d.kind==='uniform')out.uniforms.push(d);else if(d.kind==='attribute'||(type===0x8b31&&d.kind==='in'))out.attributes.push(d);else out.varyings.push(d);}return out;}
function _compileShader(shader){const s=_webglState(this),r=_webglResource(s,shader,'shader',false);if(!r)return;const scan=_scanShader(r.source||'',s.version,r.type);Object.assign(r,{compiled:scan.ok,log:scan.log,scan,digest:scan.digest});}
function _attachShader(program,shader){const s=_webglState(this),p=_webglResource(s,program,'program',false),sh=_webglResource(s,shader,'shader',false);if(!p||!sh)return;if(!p.shaders.includes(shader))p.shaders.push(shader);}
function _linkProgram(program){const s=_webglState(this),p=_webglResource(s,program,'program',false);if(!p)return;const shaders=p.shaders.map(v=>_resourceSlots.get(v)),vs=shaders.find(v=>v.type===0x8b31&&v.compiled),fs=shaders.find(v=>v.type===0x8b30&&v.compiled);p.linkGeneration++;p.uniformLocations=new Map();p.uniformValues=new Map();p.uniforms=[];p.attributes=[];if(!vs||!fs){p.linked=false;p.log='Program link failed: a compiled vertex and fragment shader are required.';return;}const uniformMap=new Map();for(const sh of [vs,fs])for(const u of sh.scan.uniforms){const old=uniformMap.get(u.name);if(old&&(old.type!==u.type||old.size!==u.size)&&!sh.scan.conditional&&!vs.scan.conditional&&!fs.scan.conditional){p.linked=false;p.log='Program link failed: conflicting uniform '+u.name+'.';return;}if(!old)uniformMap.set(u.name,u);}p.uniforms=Array.from(uniformMap.values()).sort((a,b)=>a.name.localeCompare(b.name));p.attributes=vs.scan.attributes.slice().sort((a,b)=>a.name.localeCompare(b.name));p.linked=true;p.validated=false;p.log='';p.digest=_graphicsDigest([vs.digest,fs.digest,p.uniforms,p.attributes]);}
function _shaderParameter(shader,pname){const s=_webglState(this),r=_webglResource(s,shader,'shader',false);if(!r)return null;if(pname===0x8b4f)return r.type;if(pname===0x8b81)return !!r.compiled;if(pname===0x8b80)return !!r.deleted;_webglPushError(s,0x0500);return null;}
function _programParameter(program,pname){const s=_webglState(this),r=_webglResource(s,program,'program',false);if(!r)return null;if(pname===0x8b82)return !!r.linked;if(pname===0x8b83)return !!r.validated;if(pname===0x8b80)return !!r.deleted;if(pname===0x8b85)return r.shaders.length;if(pname===0x8b86)return r.uniforms.length;if(pname===0x8b89)return r.attributes.length;if(pname===0x8a36||pname===0x8c83)return 0;if(pname===0x8c7f)return 0x8c8c;if(pname===0x91b1)return true;_webglPushError(s,0x0500);return null;}
const _glslTypes={float:0x1406,vec2:0x8b50,vec3:0x8b51,vec4:0x8b52,int:0x1404,ivec2:0x8b53,ivec3:0x8b54,ivec4:0x8b55,bool:0x8b56,mat2:0x8b5a,mat3:0x8b5b,mat4:0x8b5c,sampler2D:0x8b5e,samplerCube:0x8b60};
function _activeInfo(program,index,which){const s=_webglState(this),r=_webglResource(s,program,'program',false);if(!r)return null;const d=r[which][Number(index)];return d?new WebGLActiveInfo(_graphicsObjectToken,d.size,_glslTypes[d.type]||0x1406,d.name+(d.size>1?'[0]':'')):null;}
function _attribLocation(program,name){const s=_webglState(this),r=_webglResource(s,program,'program',false);if(!r||!r.linked)return -1;name=String(name);if(r.attribBindings.has(name))return r.attribBindings.get(name);return r.attributes.findIndex(a=>a.name===name);}
function _uniformLocation(program,name){const s=_webglState(this),r=_webglResource(s,program,'program',false);if(!r||!r.linked)return null;name=String(name).replace(/\[0\]$/,'');if(!r.uniforms.some(v=>v.name===name))return null;if(r.uniformLocations.has(name))return r.uniformLocations.get(name);const loc=_newWebglResource(WebGLUniformLocation,s,'uniformLocation',{program,programGeneration:r.linkGeneration,name});r.uniformLocations.set(name,loc);return loc;}
function _setUniform(location,values){const s=_webglState(this);if(location===null)return;const r=_webglResource(s,location,'uniformLocation',false);if(!r)return;const p=_resourceSlots.get(r.program);if(!p||p.linkGeneration!==r.programGeneration||s.currentProgram!==r.program){_webglPushError(s,0x0502);return;}p.uniformValues.set(r.name,values.map(v=>ArrayBuffer.isView(v)?Array.from(v):v));}
function _useProgram(program){const s=_webglState(this);if(program===null){s.currentProgram=null;return;}const r=_webglResource(s,program,'program',false);if(!r)return;if(!r.linked){_webglPushError(s,0x0502);return;}s.currentProgram=program;}

function _bindTexture(target,texture){const s=_webglState(this);target=Number(target);if(![0x0de1,0x8513,0x806f,0x8c1a].includes(target)){_webglPushError(s,0x0500);return;}if(texture!==null){const r=_webglResource(s,texture,'texture',false);if(!r)return;if(r.target&&r.target!==target){_webglPushError(s,0x0502);return;}r.target=target;}if(!s.textureUnits[s.activeTexture])s.textureUnits[s.activeTexture]={};const pname=target===0x0de1?0x8069:target===0x8513?0x8514:target===0x806f?0x806a:0x8c1d;s.textureUnits[s.activeTexture][pname]=texture;}
function _boundTexture(s,target){const unit=s.textureUnits[s.activeTexture]||{},cube=target===0x8513||(target>=0x8515&&target<=0x851a),p=target===0x0de1?0x8069:cube?0x8514:target===0x806f?0x806a:0x8c1d,t=unit[p];if(!t){_webglPushError(s,0x0502);return null;}return _resourceSlots.get(t);}
function _texImage2D(){const s=_webglState(this),a=arguments,target=Number(a[0]),r=_boundTexture(s,target);if(!r)return;let width,height,pixels;if(a.length>=9){width=Number(a[3]);height=Number(a[4]);pixels=a[8];}else{const source=a[5];const cs=source&&_canvasSlots.get(source);width=cs?cs.width:Number(source&&source.width)||0;height=cs?cs.height:Number(source&&source.height)||0;pixels=source&&source.data;}const max=_webglValue(s.component.parameters&&s.component.parameters['3379'])||16384;if(!Number.isSafeInteger(width)||!Number.isSafeInteger(height)||width<0||height<0||width>max||height>max){_webglPushError(s,0x0501);return;}r.width=width;r.height=height;r.level=Number(a[1]);r.format=Number(a.length>=9?a[6]:a[3]);r.type=Number(a.length>=9?a[7]:a[4]);r.surface=_newSurface(width,height);const bytes=_graphicsBytes(pixels);if(bytes)r.digest=_graphicsHashBytes(bytes).join(':');else r.digest=_graphicsDigest([width,height,r.format,r.type]);}
function _texParameter(target,pname,value){const s=_webglState(this),r=_boundTexture(s,Number(target));if(!r)return;r.parameters.set(Number(pname),Number(value));r.digest=_graphicsDigest([r.digest,pname,value]);}
function _bindFramebuffer(target,fb){const s=_webglState(this);target=Number(target);if(![0x8d40,0x8ca8,0x8ca9].includes(target)){_webglPushError(s,0x0500);return;}if(fb!==null&&_webglResource(s,fb,'framebuffer',false)===undefined)return;if(target===0x8d40||target===0x8ca8)s.readFramebuffer=fb;if(target===0x8d40||target===0x8ca9)s.drawFramebuffer=fb;}
function _framebufferTexture(target,attachment,textarget,texture,level){const s=_webglState(this),fb=(Number(target)===0x8ca8?s.readFramebuffer:s.drawFramebuffer);if(!fb){_webglPushError(s,0x0502);return;}const fr=_webglResource(s,fb,'framebuffer',false);if(!fr)return;if(texture!==null&&_webglResource(s,texture,'texture',false)===undefined)return;if(texture===null)fr.attachments.delete(Number(attachment));else fr.attachments.set(Number(attachment),{object:texture,target:Number(textarget),level:Number(level)});}
function _framebufferStatus(target){const s=_webglState(this),fb=Number(target)===0x8ca8?s.readFramebuffer:s.drawFramebuffer;if(!fb)return 0x8cd5;const r=_resourceSlots.get(fb);if(!r||!r.attachments.size)return 0x8cd7;for(const a of r.attachments.values()){const ar=_resourceSlots.get(a.object);if(!ar||ar.deleted||!ar.width||!ar.height)return 0x8cd6;}return 0x8cd5;}
function _bindRenderbuffer(target,rb){const s=_webglState(this);if(Number(target)!==0x8d41){_webglPushError(s,0x0500);return;}if(rb!==null&&_webglResource(s,rb,'renderbuffer',false)===undefined)return;s.renderbuffer=rb;}
function _renderbufferStorage(target,format,width,height){const s=_webglState(this);if(!s.renderbuffer){_webglPushError(s,0x0502);return;}const r=_resourceSlots.get(s.renderbuffer);width=Number(width);height=Number(height);if(width<0||height<0){_webglPushError(s,0x0501);return;}r.format=Number(format);r.width=width;r.height=height;r.surface=_newSurface(width,height);}
function _framebufferRenderbuffer(target,attachment,renderTarget,rb){const s=_webglState(this),fb=Number(target)===0x8ca8?s.readFramebuffer:s.drawFramebuffer;if(!fb){_webglPushError(s,0x0502);return;}const fr=_resourceSlots.get(fb);if(rb!==null&&_webglResource(s,rb,'renderbuffer',false)===undefined)return;if(rb===null)fr.attachments.delete(Number(attachment));else fr.attachments.set(Number(attachment),{object:rb,target:Number(renderTarget),level:0});}

function _webglClear(mask){const s=_webglState(this);if(s.lost)return;mask=Number(mask);if(mask&~(0x4000|0x0100|0x0400)){_webglPushError(s,0x0501);return;}if(mask&0x4000){const surface=_webglSurface(s,false),sc=s.enabled.has(0x0c11)?s.dynamic.get(0x0c10):[0,0,surface.width,surface.height],f=s.dynamic.get(0x0c22)||[0,0,0,0],m=s.dynamic.get(0x0c23)||[true,true,true,true],color=[Math.round(Math.max(0,Math.min(1,f[0]))*255),Math.round(Math.max(0,Math.min(1,f[1]))*255),Math.round(Math.max(0,Math.min(1,f[2]))*255),Math.round(Math.max(0,Math.min(1,f[3]))*255)];_surfaceRegion(surface,{kind:'clear',x:Math.max(0,sc[0]),y:Math.max(0,sc[1]),w:Math.max(0,Math.min(surface.width,sc[0]+sc[2])-Math.max(0,sc[0])),h:Math.max(0,Math.min(surface.height,sc[1]+sc[3])-Math.max(0,sc[1])),color,mask:Array.from(m)});}}
function _webglDraw(method,args){const s=_webglState(this);if(s.lost)return;if(!s.currentProgram||!(_resourceSlots.get(s.currentProgram)||{}).linked){_webglPushError(s,0x0502);return;}const surface=_webglSurface(s,false),viewport=s.dynamic.get(0x0ba2)||[0,0,surface.width,surface.height],sc=s.enabled.has(0x0c11)?s.dynamic.get(0x0c10):viewport,p=_resourceSlots.get(s.currentProgram),bindings=[];for(const [k,v] of s.bindings){const r=v&&_resourceSlots.get(v);bindings.push([k,r&&r.digest]);}bindings.sort((a,b)=>a[0]-b[0]);const transcript=[_fingerprintProfile&&_fingerprintProfile.id,_fingerprintProfile&&_fingerprintProfile.renderSeed,s.generation,p.digest,Array.from(p.uniformValues.entries()).sort(),bindings,Array.from(s.enabled).sort(),Array.from(viewport),Array.from(sc),method,args,s.drawNumber++];const hash=_graphicsHashText(JSON.stringify(transcript));_surfaceRegion(surface,{kind:'draw',x:Math.max(0,sc[0]),y:Math.max(0,sc[1]),w:Math.max(0,Math.min(surface.width,sc[0]+sc[2])-Math.max(0,sc[0])),h:Math.max(0,Math.min(surface.height,sc[1]+sc[3])-Math.max(0,sc[1])),hash,mask:Array.from(s.dynamic.get(0x0c23)||[true,true,true,true])});s.commands.push(hash);if(s.commands.length>_GRAPHICS_COMMAND_LIMIT)s.commands.splice(0,s.commands.length-_GRAPHICS_COMMAND_LIMIT);}
function _readPixels(x,y,width,height,format,type,dst,dstOffset){
  const s=_webglState(this);x=Number(x);y=Number(y);width=Number(width);height=Number(height);format=Number(format);type=Number(type);
  if(!Number.isSafeInteger(x)||!Number.isSafeInteger(y)||!Number.isSafeInteger(width)||!Number.isSafeInteger(height)||width<0||height<0){_webglPushError(s,0x0501);return;}
  const componentsByFormat={0x1903:1,0x8227:2,0x1907:3,0x1908:4,0x8d94:1,0x8228:2,0x8d98:3,0x8d99:4};
  const components=componentsByFormat[format];if(!components){_webglPushError(s,0x0500);return;}
  const packed=(type===0x8033||type===0x8034||type===0x8363||type===0x8368);
  if((type===0x8033||type===0x8034||type===0x8368)&&format!==0x1908){_webglPushError(s,0x0502);return;}
  if(type===0x8363&&format!==0x1907){_webglPushError(s,0x0502);return;}
  const integerFormat=(format===0x8d94||format===0x8228||format===0x8d98||format===0x8d99);
  const typeInfo={
    0x1400:[1,Int8Array],0x1401:[1,Uint8Array],0x1402:[2,Int16Array],0x1403:[2,Uint16Array],
    0x1404:[4,Int32Array],0x1405:[4,Uint32Array],0x1406:[4,Float32Array],0x140b:[2,Uint16Array],
    0x8033:[2,Uint16Array],0x8034:[2,Uint16Array],0x8363:[2,Uint16Array],0x8368:[4,Uint32Array]
  }[type];
  if(!typeInfo){_webglPushError(s,0x0500);return;}
  if(integerFormat&&(type===0x1406||type===0x140b||packed)){_webglPushError(s,0x0502);return;}
  if(!integerFormat&&(type===0x1400||type===0x1402||type===0x1404||type===0x1405)&&s.version===1){_webglPushError(s,0x0502);return;}
  const elementBytes=typeInfo[0],pixelBytes=packed?elementBytes:components*elementBytes;
  const surface=_webglSurface(s,true),pack=Number(s.dynamic.get(0x0d05)||4),rowLength=s.version===2?Number(s.dynamic.get(0x0d02)||0):0,skipPixels=s.version===2?Number(s.dynamic.get(0x0d04)||0):0,skipRows=s.version===2?Number(s.dynamic.get(0x0d03)||0):0;
  if(rowLength<0||skipPixels<0||skipRows<0||(rowLength&&rowLength<width)){_webglPushError(s,0x0502);return;}
  const rowPixels=rowLength||width,rowBytes=rowPixels*pixelBytes,stride=Math.ceil(rowBytes/pack)*pack,span=height?(skipRows+height-1)*stride+(skipPixels+width)*pixelBytes:0,packBuffer=s.bindings.get(0x88eb);
  if(!Number.isSafeInteger(span)||span>_GRAPHICS_PIXEL_WORK_LIMIT){_webglPushError(s,0x0505);return;}
  let out,base;
  if(packBuffer){
    base=Number(dst)||0;const r=_resourceSlots.get(packBuffer);
    if(!Number.isSafeInteger(base)||base<0||!r||base+span>r.size){_webglPushError(s,0x0502);return;}
    if(!r.bytes){if(r.size>_WEBGL_SHADOW_LIMIT||s.shadowBytes+r.size>_WEBGL_SHADOW_LIMIT){_webglPushError(s,0x0505);return;}r.bytes=new Uint8Array(r.size);s.shadowBytes+=r.size;}
    out=r.bytes;
  }else{
    if(!(dst instanceof typeInfo[1])){_webglPushError(s,0x0502);return;}
    out=_graphicsBytes(dst);base=(Number(dstOffset)||0)*elementBytes;
    if(!Number.isSafeInteger(base)||base<0||base+span>out.byteLength){_webglPushError(s,0x0502);return;}
  }
  const view=new DataView(out.buffer,out.byteOffset,out.byteLength),little=true;
  const half=value=>{if(!value)return 0;const exponent=Math.floor(Math.log2(value)),halfExponent=exponent+15;if(halfExponent<=0)return Math.round(value/Math.pow(2,-24));if(halfExponent>=31)return 0x7c00;let mantissa=Math.round((value/Math.pow(2,exponent)-1)*1024),adjusted=halfExponent;if(mantissa===1024){mantissa=0;adjusted++;}return adjusted>=31?0x7c00:(adjusted<<10)|(mantissa&1023);};
  const write=(at,color)=>{
    if(type===0x8033){view.setUint16(at,((color[0]>>>4)<<12)|((color[1]>>>4)<<8)|((color[2]>>>4)<<4)|(color[3]>>>4),little);return;}
    if(type===0x8034){view.setUint16(at,((color[0]>>>3)<<11)|((color[1]>>>3)<<6)|((color[2]>>>3)<<1)|(color[3]>=128?1:0),little);return;}
    if(type===0x8363){view.setUint16(at,((color[0]>>>3)<<11)|((color[1]>>>2)<<5)|(color[2]>>>3),little);return;}
    if(type===0x8368){view.setUint32(at,((color[3]>>>6)<<30)|((color[2]*1023/255&1023)<<20)|((color[1]*1023/255&1023)<<10)|(color[0]*1023/255&1023),little);return;}
    for(let index=0;index<components;index++){
      const value=color[index],offset=at+index*elementBytes;
      if(type===0x1401)view.setUint8(offset,value);else if(type===0x1400)view.setInt8(offset,Math.round(value/255*127));
      else if(type===0x1403)view.setUint16(offset,value*257,little);else if(type===0x1402)view.setInt16(offset,Math.round(value/255*32767),little);
      else if(type===0x1405)view.setUint32(offset,Math.round(value/255*4294967295),little);else if(type===0x1404)view.setInt32(offset,Math.round(value/255*2147483647),little);
      else if(type===0x1406)view.setFloat32(offset,value/255,little);else view.setUint16(offset,half(value/255),little);
    }
  };
  for(let row=0;row<height;row++)for(let col=0;col<width;col++){
    const at=base+(skipRows+row)*stride+(skipPixels+col)*pixelBytes;
    const color=x+col>=0&&y+row>=0&&x+col<surface.width&&y+row<surface.height?_surfacePixel(surface,x+col,y+row):[0,0,0,0];write(at,color);
  }
}

function _vertexAttrib(s,index){index=Number(index);const max=_webglValue(s.component.parameters&&s.component.parameters['34921'])||16;if(!Number.isInteger(index)||index<0||index>=max){_webglPushError(s,0x0501);return null;}if(!s.vertexAttribs.has(index))s.vertexAttribs.set(index,{enabled:false,size:4,type:0x1406,normalized:false,stride:0,offset:0,integer:false,divisor:0,buffer:null,current:new Float32Array([0,0,0,1])});return s.vertexAttribs.get(index);}
function _vertexAttribPointer(index,size,type,normalized,stride,offset,integer){const s=_webglState(this),a=_vertexAttrib(s,index);if(!a)return;if(!s.bindings.get(0x8892)){_webglPushError(s,0x0502);return;}size=Number(size);stride=Number(stride);offset=Number(offset);if(size<1||size>4||stride<0||offset<0){_webglPushError(s,0x0501);return;}Object.assign(a,{size,type:Number(type),normalized:!!normalized,stride,offset,integer:!!integer,buffer:s.bindings.get(0x8892)});}
function _getVertexAttrib(index,pname){const s=_webglState(this),a=_vertexAttrib(s,index);if(!a)return null;const map={0x8622:'enabled',0x8623:'size',0x8624:'stride',0x8625:'type',0x886a:'normalized',0x889f:'buffer',0x88fd:'integer',0x88fe:'divisor'};if(Number(pname)===0x8626)return new Float32Array(a.current);const key=map[Number(pname)];if(key)return a[key];_webglPushError(s,0x0500);return null;}
function _setDynamicCall(name,enums){return function(){const s=_webglState(this),args=Array.from(arguments,Number);for(let i=0;i<enums.length;i++)s.dynamic.set(enums[i],args[Math.min(i,args.length-1)]);s.commands.push(_graphicsHashText(name+JSON.stringify(args)));};}

function _canvasGetContext(canvas, type, options) {
  const s=_canvasSlots.get(canvas);type=String(type).toLowerCase();let mode=type==='experimental-webgl'?'webgl':type;
  if(!['2d','webgl','webgl2','webgpu'].includes(mode))return null;
  if(s.mode){return s.mode===mode?s.context:null;}
  if(mode==='2d'){
    const bytes=s.width*s.height*4;if(!Number.isSafeInteger(bytes)||bytes>_GRAPHICS_PIXEL_WORK_LIMIT)return null;
    s.context=new _Canvas2D(canvas);s.mode=mode;return s.context;
  }
  // Fork: the accelerated contexts exist only once a fingerprint profile is
  // loaded. Every parameter they report (unmasked vendor and renderer, limits,
  // extension list, draw digests) is read from that profile, so without one
  // this would be a context backed by nothing, which is precisely what
  // upstream refuses to hand out. With no profile we return null exactly as
  // upstream does; the facade appears only when it can be answered truthfully.
  if(!(_fingerprintProfile&&_fingerprintProfile.graphics))return null;
  if(mode==='webgl'||mode==='webgl2'){const C=mode==='webgl2'?WebGL2RenderingContext:WebGLRenderingContext;s.context=new C(_graphicsObjectToken,canvas,options||{});s.mode=mode;return s.context;}
  s.context=new GPUCanvasContext(_graphicsObjectToken,canvas);s.mode=mode;return s.context;
}

const _webglMethods={
  getContextAttributes:_webglContextAttributes,getParameter:_webglGetParameter,getError:_webglGetError,getSupportedExtensions:_webglSupportedExtensions,getExtension:_webglExtension,getShaderPrecisionFormat:_webglPrecision,isContextLost:function(){return _webglState(this).lost;},
  createBuffer:_createResourceMethod(WebGLBuffer,'buffer',()=>({size:0,usage:0,bytes:null})),createTexture:_createResourceMethod(WebGLTexture,'texture',()=>({target:0,parameters:new Map(),width:0,height:0,surface:null})),createFramebuffer:_createResourceMethod(WebGLFramebuffer,'framebuffer',()=>({attachments:new Map()})),createRenderbuffer:_createResourceMethod(WebGLRenderbuffer,'renderbuffer',()=>({width:0,height:0})),createShader:function(type){const s=_webglState(this);type=Number(type);if(type!==0x8b31&&type!==0x8b30){_webglPushError(s,0x0500);return null;}return _newWebglResource(WebGLShader,s,'shader',{type,source:'',compiled:false,log:'',scan:null});},createProgram:_createResourceMethod(WebGLProgram,'program',()=>({shaders:[],linked:false,validated:false,log:'',uniforms:[],attributes:[],uniformLocations:new Map(),uniformValues:new Map(),attribBindings:new Map(),linkGeneration:0})),
  deleteBuffer:function(v){_deleteResource(_webglState(this),v,'buffer');},deleteTexture:function(v){_deleteResource(_webglState(this),v,'texture');},deleteFramebuffer:function(v){_deleteResource(_webglState(this),v,'framebuffer');},deleteRenderbuffer:function(v){_deleteResource(_webglState(this),v,'renderbuffer');},deleteShader:function(v){_deleteResource(_webglState(this),v,'shader');},deleteProgram:function(v){_deleteResource(_webglState(this),v,'program');},
  isBuffer:function(v){return _isResource(_webglState(this),v,'buffer');},isTexture:function(v){return _isResource(_webglState(this),v,'texture');},isFramebuffer:function(v){return _isResource(_webglState(this),v,'framebuffer');},isRenderbuffer:function(v){return _isResource(_webglState(this),v,'renderbuffer');},isShader:function(v){return _isResource(_webglState(this),v,'shader');},isProgram:function(v){return _isResource(_webglState(this),v,'program');},
  bindBuffer:_bindBuffer,bufferData:_bufferData,bufferSubData:_bufferSubData,getBufferParameter:_getBufferParameter,
  shaderSource:_shaderSource,compileShader:_compileShader,attachShader:_attachShader,detachShader:function(program,shader){const s=_webglState(this),p=_webglResource(s,program,'program',false);if(p)p.shaders=p.shaders.filter(v=>v!==shader);},linkProgram:_linkProgram,useProgram:_useProgram,validateProgram:function(program){const s=_webglState(this),p=_webglResource(s,program,'program',false);if(p)p.validated=!!p.linked;},getShaderParameter:_shaderParameter,getProgramParameter:_programParameter,getShaderInfoLog:function(shader){const s=_webglState(this),r=_webglResource(s,shader,'shader',false);return r?r.log:null;},getProgramInfoLog:function(program){const s=_webglState(this),r=_webglResource(s,program,'program',false);return r?r.log:null;},getShaderSource:function(shader){const s=_webglState(this),r=_webglResource(s,shader,'shader',false);return r?r.source:null;},getAttachedShaders:function(program){const s=_webglState(this),r=_webglResource(s,program,'program',false);return r?r.shaders.slice():null;},getActiveUniform:function(p,i){return _activeInfo.call(this,p,i,'uniforms');},getActiveAttrib:function(p,i){return _activeInfo.call(this,p,i,'attributes');},bindAttribLocation:function(program,index,name){const s=_webglState(this),p=_webglResource(s,program,'program',false);if(p)p.attribBindings.set(String(name),Number(index));},getAttribLocation:_attribLocation,getUniformLocation:_uniformLocation,getUniform:function(program,location){const s=_webglState(this),p=_webglResource(s,program,'program',false),l=_webglResource(s,location,'uniformLocation',false);return p&&l?p.uniformValues.get(l.name)??null:null;},
  bindTexture:_bindTexture,activeTexture:function(texture){const s=_webglState(this),n=Number(texture)-0x84c0;if(n<0||n>=32){_webglPushError(s,0x0500);return;}s.activeTexture=n;s.dynamic.set(0x84e0,Number(texture));},texImage2D:_texImage2D,texSubImage2D:function(){const s=_webglState(this);const bytes=_graphicsBytes(arguments[arguments.length-1]);const r=_boundTexture(s,Number(arguments[0]));if(r&&bytes)r.digest=_graphicsDigest([r.digest,_graphicsHashBytes(bytes)]);},texParameteri:_texParameter,texParameterf:_texParameter,getTexParameter:function(target,pname){const s=_webglState(this),r=_boundTexture(s,Number(target));return r?(r.parameters.get(Number(pname))??null):null;},generateMipmap:function(target){const s=_webglState(this),r=_boundTexture(s,Number(target));if(r)r.mipmapped=true;},
  bindFramebuffer:_bindFramebuffer,framebufferTexture2D:_framebufferTexture,checkFramebufferStatus:_framebufferStatus,bindRenderbuffer:_bindRenderbuffer,renderbufferStorage:_renderbufferStorage,framebufferRenderbuffer:_framebufferRenderbuffer,
  clear:_webglClear,clearColor:function(r,g,b,a){_webglState(this).dynamic.set(0x0c22,new Float32Array([Number(r),Number(g),Number(b),Number(a)]));},clearDepth:function(v){_webglState(this).dynamic.set(0x0b73,Math.max(0,Math.min(1,Number(v))));},clearStencil:function(v){_webglState(this).dynamic.set(0x0b91,Number(v)|0);},colorMask:function(r,g,b,a){_webglState(this).dynamic.set(0x0c23,[!!r,!!g,!!b,!!a]);},depthMask:function(v){_webglState(this).dynamic.set(0x0b72,!!v);},viewport:function(x,y,w,h){const s=_webglState(this);if(w<0||h<0){_webglPushError(s,0x0501);return;}s.dynamic.set(0x0ba2,new Int32Array([x|0,y|0,w|0,h|0]));},scissor:function(x,y,w,h){const s=_webglState(this);if(w<0||h<0){_webglPushError(s,0x0501);return;}s.dynamic.set(0x0c10,new Int32Array([x|0,y|0,w|0,h|0]));},enable:function(cap){const s=_webglState(this);cap=Number(cap);if(![0x0be2,0x0b44,0x0b71,0x0bd0,0x0c11,0x8037,0x809e,0x8c89].includes(cap)){_webglPushError(s,0x0500);return;}s.enabled.add(cap);s.dynamic.set(cap,true);},disable:function(cap){const s=_webglState(this);s.enabled.delete(Number(cap));s.dynamic.set(Number(cap),false);},isEnabled:function(cap){return _webglState(this).enabled.has(Number(cap));},pixelStorei:function(pname,value){const s=_webglState(this),p=Number(pname),v=Number(value);const valid=[0x0d05,0x0cf5,0x0cf2,0x0cf3,0x0cf4,0x806e,0x0d02,0x0d03,0x0d04,0x9240,0x9241,0x9243];if(!valid.includes(p)){_webglPushError(s,0x0500);return;}if((p===0x0d05||p===0x0cf5)&&![1,2,4,8].includes(v)){_webglPushError(s,0x0501);return;}s.dynamic.set(p,(p>=0x9240&&p<=0x9241)?!!value:v);},readPixels:_readPixels,
  enableVertexAttribArray:function(index){const a=_vertexAttrib(_webglState(this),index);if(a)a.enabled=true;},disableVertexAttribArray:function(index){const a=_vertexAttrib(_webglState(this),index);if(a)a.enabled=false;},vertexAttribPointer:function(index,size,type,normalized,stride,offset){_vertexAttribPointer.call(this,index,size,type,normalized,stride,offset,false);},vertexAttribIPointer:function(index,size,type,stride,offset){_vertexAttribPointer.call(this,index,size,type,false,stride,offset,true);},getVertexAttrib:_getVertexAttrib,getVertexAttribOffset:function(index,pname){const a=_vertexAttrib(_webglState(this),index);return a?a.offset:0;},vertexAttribDivisor:function(index,value){const a=_vertexAttrib(_webglState(this),index);if(a)a.divisor=Number(value);},
  drawArrays:function(a,b,c){_webglDraw.call(this,'drawArrays',[a,b,c]);},drawElements:function(a,b,c,d){_webglDraw.call(this,'drawElements',[a,b,c,d]);},flush:function(){},finish:function(){},
};
for(const name of ['uniform1f','uniform1fv','uniform1i','uniform1iv','uniform2f','uniform2fv','uniform2i','uniform2iv','uniform3f','uniform3fv','uniform3i','uniform3iv','uniform4f','uniform4fv','uniform4i','uniform4iv','uniformMatrix2fv','uniformMatrix3fv','uniformMatrix4fv','uniform1ui','uniform1uiv','uniform2ui','uniform2uiv','uniform3ui','uniform3uiv','uniform4ui','uniform4uiv','uniformMatrix2x3fv','uniformMatrix2x4fv','uniformMatrix3x2fv','uniformMatrix3x4fv','uniformMatrix4x2fv','uniformMatrix4x3fv'])_webglMethods[name]=function(location){_setUniform.call(this,location,Array.prototype.slice.call(arguments,1));};
Object.assign(_webglMethods,{
  blendColor:function(r,g,b,a){_webglState(this).dynamic.set(0x8005,new Float32Array([r,g,b,a]));},
  blendEquation:_setDynamicCall('blendEquation',[0x8009,0x883d]),blendEquationSeparate:_setDynamicCall('blendEquationSeparate',[0x8009,0x883d]),
  blendFunc:function(src,dst){const s=_webglState(this);s.dynamic.set(0x80c9,Number(src));s.dynamic.set(0x80cb,Number(src));s.dynamic.set(0x80c8,Number(dst));s.dynamic.set(0x80ca,Number(dst));},blendFuncSeparate:_setDynamicCall('blendFuncSeparate',[0x80c9,0x80c8,0x80cb,0x80ca]),
  cullFace:_setDynamicCall('cullFace',[0x0b45]),depthFunc:_setDynamicCall('depthFunc',[0x0b74]),frontFace:_setDynamicCall('frontFace',[0x0b46]),lineWidth:_setDynamicCall('lineWidth',[0x0b21]),
  depthRange:function(n,f){_webglState(this).dynamic.set(0x0b70,new Float32Array([Math.max(0,Math.min(1,Number(n))),Math.max(0,Math.min(1,Number(f)))]));},
  polygonOffset:function(f,u){const s=_webglState(this);s.dynamic.set(0x8038,Number(f));s.dynamic.set(0x2a00,Number(u));},sampleCoverage:function(v,i){const s=_webglState(this);s.dynamic.set(0x80aa,Math.max(0,Math.min(1,Number(v))));s.dynamic.set(0x80ab,!!i);},
  hint:function(){},stencilFunc:_setDynamicCall('stencilFunc',[0x0b92,0x0b97,0x0b93]),stencilFuncSeparate:function(){},stencilMask:_setDynamicCall('stencilMask',[0x0b98]),stencilMaskSeparate:function(){},stencilOp:_setDynamicCall('stencilOp',[0x0b94,0x0b95,0x0b96]),stencilOpSeparate:function(){},
});

function _installWebglMethods(C,manifest){for(const name of Object.keys(manifest)){let fn=_webglMethods[name];if(!fn)fn=function(){const s=_webglState(this);if(s.lost)return null;return undefined;};_graphicsDefineMethod(C.prototype,name,manifest[name],fn);}}
_installWebglMethods(WebGLRenderingContext,_WEBGL1_METHODS);
_installWebglMethods(WebGL2RenderingContext,_WEBGL2_METHODS);
for(const C of [WebGLRenderingContext,WebGL2RenderingContext])_graphicsDefineProperties(C.prototype,{
  canvas:{get:function(){return _webglState(this).canvas;},enumerable:true,configurable:true},
  drawingBufferWidth:{get:function(){return Math.max(1,_canvasSize(_webglState(this).canvas)[0]);},enumerable:true,configurable:true},
  drawingBufferHeight:{get:function(){return Math.max(1,_canvasSize(_webglState(this).canvas)[1]);},enumerable:true,configurable:true},
  drawingBufferColorSpace:{get:function(){return _webglState(this).drawingBufferColorSpace;},set:function(v){const n=String(v);if(n!=='srgb'&&n!=='display-p3')throw new TypeError('The color space is not supported.');_webglState(this).drawingBufferColorSpace=n;},enumerable:true,configurable:true},
  unpackColorSpace:{get:function(){return _webglState(this).unpackColorSpace;},set:function(v){const n=String(v);if(n!=='srgb'&&n!=='display-p3')throw new TypeError('The color space is not supported.');_webglState(this).unpackColorSpace=n;},enumerable:true,configurable:true},
});

// WebGL 2 objects and commands which are not part of the shared WebGL state.
const _webgl2Extra = {
  createVertexArray:_createResourceMethod(WebGLVertexArrayObject,'vertexArray',()=>({attributes:new Map(),elementBuffer:null})),
  deleteVertexArray:function(v){_deleteResource(_webglState(this),v,'vertexArray');},
  isVertexArray:function(v){return _isResource(_webglState(this),v,'vertexArray');},
  bindVertexArray:function(v){const s=_webglState(this);if(v===null||_webglResource(s,v,'vertexArray',true)!==undefined)s.vao=v;},
  createQuery:_createResourceMethod(WebGLQuery,'query',()=>({active:false,available:false,result:0,target:0})),
  deleteQuery:function(v){_deleteResource(_webglState(this),v,'query');},
  isQuery:function(v){return _isResource(_webglState(this),v,'query');},
  beginQuery:function(target,q){const s=_webglState(this),r=_webglResource(s,q,'query',false);if(!r)return;if(s.query){_webglPushError(s,0x0502);return;}r.active=true;r.target=Number(target);r.available=false;s.query=q;},
  endQuery:function(target){const s=_webglState(this);if(!s.query){_webglPushError(s,0x0502);return;}const r=_resourceSlots.get(s.query);r.active=false;r.available=true;r.result=Math.max(1,s.drawNumber);s.query=null;},
  getQuery:function(target,pname){const s=_webglState(this);return Number(pname)===0x8865?s.query:null;},
  getQueryParameter:function(q,pname){const s=_webglState(this),r=_webglResource(s,q,'query',false);if(!r)return null;if(Number(pname)===0x8867)return r.available;if(Number(pname)===0x8866)return r.result;_webglPushError(s,0x0500);return null;},
  createSampler:_createResourceMethod(WebGLSampler,'sampler',()=>({parameters:new Map()})),
  deleteSampler:function(v){_deleteResource(_webglState(this),v,'sampler');},
  isSampler:function(v){return _isResource(_webglState(this),v,'sampler');},
  bindSampler:function(unit,sampler){const s=_webglState(this);unit=Number(unit);if(unit<0||unit>=32){_webglPushError(s,0x0501);return;}if(sampler!==null&&_webglResource(s,sampler,'sampler',false)===undefined)return;if(!s.textureUnits[unit])s.textureUnits[unit]={};s.textureUnits[unit].sampler=sampler;},
  samplerParameteri:function(sampler,pname,value){const s=_webglState(this),r=_webglResource(s,sampler,'sampler',false);if(r)r.parameters.set(Number(pname),Number(value));},
  samplerParameterf:function(sampler,pname,value){return _webgl2Extra.samplerParameteri.call(this,sampler,pname,value);},
  getSamplerParameter:function(sampler,pname){const s=_webglState(this),r=_webglResource(s,sampler,'sampler',false);return r?(r.parameters.get(Number(pname))??null):null;},
  createTransformFeedback:_createResourceMethod(WebGLTransformFeedback,'transformFeedback',()=>({active:false,paused:false,bindings:new Map()})),
  deleteTransformFeedback:function(v){_deleteResource(_webglState(this),v,'transformFeedback');},
  isTransformFeedback:function(v){return _isResource(_webglState(this),v,'transformFeedback');},
  bindTransformFeedback:function(target,v){const s=_webglState(this);if(v!==null&&_webglResource(s,v,'transformFeedback',false)===undefined)return;s.transformFeedback=v;},
  beginTransformFeedback:function(){const s=_webglState(this),r=s.transformFeedback&&_resourceSlots.get(s.transformFeedback);if(!r){_webglPushError(s,0x0502);return;}r.active=true;r.paused=false;},
  endTransformFeedback:function(){const s=_webglState(this),r=s.transformFeedback&&_resourceSlots.get(s.transformFeedback);if(!r||!r.active){_webglPushError(s,0x0502);return;}r.active=false;r.paused=false;},
  pauseTransformFeedback:function(){const s=_webglState(this),r=s.transformFeedback&&_resourceSlots.get(s.transformFeedback);if(r&&r.active)r.paused=true;else _webglPushError(s,0x0502);},
  resumeTransformFeedback:function(){const s=_webglState(this),r=s.transformFeedback&&_resourceSlots.get(s.transformFeedback);if(r&&r.active&&r.paused)r.paused=false;else _webglPushError(s,0x0502);},
  fenceSync:function(condition,flags){const s=_webglState(this);if(Number(condition)!==0x9117||Number(flags)!==0){_webglPushError(s,0x0501);return null;}return _newWebglResource(WebGLSync,s,'sync',{signaled:true,condition:Number(condition),flags:0});},
  deleteSync:function(v){_deleteResource(_webglState(this),v,'sync');},
  isSync:function(v){return _isResource(_webglState(this),v,'sync');},
  clientWaitSync:function(v,flags,timeout){const s=_webglState(this),r=_webglResource(s,v,'sync',false);return r?0x911a:0x911d;},
  waitSync:function(v){_webglResource(_webglState(this),v,'sync',false);},
  getSyncParameter:function(v,pname){const s=_webglState(this),r=_webglResource(s,v,'sync',false);if(!r)return null;if(Number(pname)===0x9114)return 0x9117;if(Number(pname)===0x9115)return 0;if(Number(pname)===0x9112)return 0x9117;if(Number(pname)===0x9113)return 0x9119;_webglPushError(s,0x0500);return null;},
  getBufferSubData:function(target,offset,dst,dstOffset,length){const s=_webglState(this),r=_boundBuffer(s,target),out=_graphicsBytes(dst);if(!r||!out)return;offset=Number(offset)||0;dstOffset=Number(dstOffset)||0;const count=length===undefined?out.byteLength-dstOffset:Number(length);if(offset<0||dstOffset<0||count<0||offset+count>r.size||dstOffset+count>out.byteLength){_webglPushError(s,0x0501);return;}if(r.bytes)out.set(r.bytes.subarray(offset,offset+count),dstOffset);else out.fill(0,dstOffset,dstOffset+count);},
  copyBufferSubData:function(readTarget,writeTarget,readOffset,writeOffset,size){const s=_webglState(this),src=_boundBuffer(s,readTarget),dst=_boundBuffer(s,writeTarget);readOffset=Number(readOffset);writeOffset=Number(writeOffset);size=Number(size);if(!src||!dst)return;if(readOffset<0||writeOffset<0||size<0||readOffset+size>src.size||writeOffset+size>dst.size){_webglPushError(s,0x0501);return;}if(src.bytes&&dst.bytes)dst.bytes.set(src.bytes.slice(readOffset,readOffset+size),writeOffset);dst.digest=_graphicsDigest([dst.digest,src.digest,readOffset,writeOffset,size]);},
  bindBufferBase:function(target,index,buffer){const s=_webglState(this);if(buffer!==null&&_webglResource(s,buffer,'buffer',false)===undefined)return;if(!s.indexedBindings)s.indexedBindings=new Map();s.indexedBindings.set(Number(target)+':'+Number(index),{buffer,offset:0,size:buffer?(_resourceSlots.get(buffer).size||0):0});},
  bindBufferRange:function(target,index,buffer,offset,size){const s=_webglState(this);if(buffer!==null&&_webglResource(s,buffer,'buffer',false)===undefined)return;if(offset<0||size<=0){_webglPushError(s,0x0501);return;}if(!s.indexedBindings)s.indexedBindings=new Map();s.indexedBindings.set(Number(target)+':'+Number(index),{buffer,offset:Number(offset),size:Number(size)});},
  getIndexedParameter:function(target,index){const s=_webglState(this),v=s.indexedBindings&&s.indexedBindings.get(Number(target)+':'+Number(index));return v?v.buffer:null;},
  drawArraysInstanced:function(a,b,c,d){_webglDraw.call(this,'drawArraysInstanced',[a,b,c,d]);},
  drawElementsInstanced:function(a,b,c,d,e){_webglDraw.call(this,'drawElementsInstanced',[a,b,c,d,e]);},
  drawRangeElements:function(a,b,c,d,e,f){_webglDraw.call(this,'drawRangeElements',[a,b,c,d,e,f]);},
  texImage3D:function(){const s=_webglState(this),r=_boundTexture(s,Number(arguments[0]));if(!r)return;const w=Number(arguments[3]),h=Number(arguments[4]),d=Number(arguments[5]);if(w<0||h<0||d<0){_webglPushError(s,0x0501);return;}r.width=w;r.height=h;r.depth=d;r.surface=_newSurface(w,h);r.digest=_graphicsDigest(Array.from(arguments).slice(0,10));},
  texStorage2D:function(target,levels,format,width,height){const s=_webglState(this),r=_boundTexture(s,Number(target));if(!r)return;r.width=Number(width);r.height=Number(height);r.levels=Number(levels);r.format=Number(format);r.immutable=true;r.surface=_newSurface(r.width,r.height);},
  texStorage3D:function(target,levels,format,width,height,depth){_webgl2Extra.texStorage2D.call(this,target,levels,format,width,height);const s=_webglState(this),r=_boundTexture(s,Number(target));if(r)r.depth=Number(depth);},
};
for(const name of Object.keys(_webgl2Extra))if(_WEBGL2_METHODS[name]!==undefined)_graphicsDefineMethod(WebGL2RenderingContext.prototype,name,_WEBGL2_METHODS[name],_webgl2Extra[name]);

// WebGPU uses private slots and records commands. It performs only the exact
// CPU work listed below. Shader draw and compute work stays logical.
const _gpuSlots = new WeakMap();
const _gpuCanvasSlots = new WeakMap();
function _gpuBrand(self,kind){const s=_gpuSlots.get(self);if(!s||(kind&&s.kind!==kind))throw new TypeError('Illegal invocation');return s;}
function _gpuClass(name) {
  const C=class{constructor(token,state){if(token!==_graphicsObjectToken)_graphicsIllegalConstructor();_gpuSlots.set(this,Object.assign({kind:name,label:'',destroyed:false},state||{}));}};
  Object.defineProperty(C,'name',{value:name});_graphicsTag(C.prototype,name);_markNative(C);_graphicsDefineGlobal(name,C);return C;
}
const _gpuClasses={};
for(const name of Object.keys(_WEBGPU_INTERFACES))_gpuClasses[name]=_gpuClass(name);
const GPU=_gpuClasses.GPU, GPUAdapter=_gpuClasses.GPUAdapter, GPUAdapterInfo=_gpuClasses.GPUAdapterInfo,
  GPUDevice=_gpuClasses.GPUDevice, GPUQueue=_gpuClasses.GPUQueue, GPUBuffer=_gpuClasses.GPUBuffer,
  GPUTexture=_gpuClasses.GPUTexture, GPUTextureView=_gpuClasses.GPUTextureView, GPUSampler=_gpuClasses.GPUSampler,
  GPUShaderModule=_gpuClasses.GPUShaderModule, GPUCommandEncoder=_gpuClasses.GPUCommandEncoder,
  GPUCommandBuffer=_gpuClasses.GPUCommandBuffer, GPURenderPassEncoder=_gpuClasses.GPURenderPassEncoder,
  GPUComputePassEncoder=_gpuClasses.GPUComputePassEncoder, GPURenderBundleEncoder=_gpuClasses.GPURenderBundleEncoder,
  GPURenderBundle=_gpuClasses.GPURenderBundle, GPURenderPipeline=_gpuClasses.GPURenderPipeline,
  GPUComputePipeline=_gpuClasses.GPUComputePipeline, GPUQuerySet=_gpuClasses.GPUQuerySet,
  GPUBindGroup=_gpuClasses.GPUBindGroup, GPUBindGroupLayout=_gpuClasses.GPUBindGroupLayout,
  GPUPipelineLayout=_gpuClasses.GPUPipelineLayout, GPUCanvasContext=_gpuClasses.GPUCanvasContext;
for(const name of ['GPUDevice','GPUQueue','GPUBuffer','GPUTexture','GPUTextureView','GPUSampler','GPUShaderModule','GPUCommandEncoder','GPUCommandBuffer','GPURenderPassEncoder','GPUComputePassEncoder','GPURenderBundleEncoder','GPURenderBundle','GPURenderPipeline','GPUComputePipeline','GPUQuerySet','GPUBindGroup','GPUBindGroupLayout','GPUPipelineLayout']){
  const C=_gpuClasses[name];_graphicsDefineProperties(C.prototype,{label:{get:function(){return _gpuBrand(this,name).label||'';},set:function(v){_gpuBrand(this,name).label=String(v);},enumerable:true,configurable:true}});
}

class _GPUSupportedSet {
  constructor(token,values,tag){if(token!==_graphicsObjectToken)_graphicsIllegalConstructor();this._set=new Set(values||[]);this._tag=tag;}
  get size(){return this._set.size;}has(v){return this._set.has(String(v));}keys(){return this._set.keys();}values(){return this._set.values();}entries(){return this._set.entries();}forEach(fn,thisArg){return this._set.forEach((v)=>fn.call(thisArg,v,v,this));}[Symbol.iterator](){return this.values();}
  get [Symbol.toStringTag](){return this._tag;}
}
class GPUSupportedFeatures extends _GPUSupportedSet {constructor(token,values){super(token,values,'GPUSupportedFeatures');}}
class WGSLLanguageFeatures extends _GPUSupportedSet {constructor(token,values){super(token,values,'WGSLLanguageFeatures');}}
class GPUSupportedLimits {constructor(token,values){if(token!==_graphicsObjectToken)_graphicsIllegalConstructor();for(const k of Object.keys(values||{}))Object.defineProperty(this,k,{value:values[k],enumerable:true});Object.freeze(this);}}
for(const C of [GPUSupportedFeatures,WGSLLanguageFeatures,GPUSupportedLimits]){_markNative(C);_graphicsDefineGlobal(C.name,C);}

class GPUError extends Error {constructor(token,message){if(token!==_graphicsObjectToken)_graphicsIllegalConstructor();super(String(message||''));this.name=this.constructor.name;}}
class GPUValidationError extends GPUError {constructor(message){super(_graphicsObjectToken,message);}}
class GPUOutOfMemoryError extends GPUError {constructor(message){super(_graphicsObjectToken,message);}}
class GPUInternalError extends GPUError {constructor(message){super(_graphicsObjectToken,message);}}
class GPUDeviceLostInfo {constructor(token,reason,message){if(token!==_graphicsObjectToken)_graphicsIllegalConstructor();this.reason=reason;this.message=message;}}
class GPUCompilationMessage {constructor(token,message,type,lineNum,linePos,offset,length){if(token!==_graphicsObjectToken)_graphicsIllegalConstructor();Object.assign(this,{message,type,lineNum,linePos,offset,length});}}
class GPUCompilationInfo {constructor(token,messages){if(token!==_graphicsObjectToken)_graphicsIllegalConstructor();this.messages=Object.freeze(messages.slice());}}
for(const C of [GPUError,GPUValidationError,GPUOutOfMemoryError,GPUInternalError,GPUDeviceLostInfo,GPUCompilationMessage,GPUCompilationInfo]){_graphicsTag(C.prototype,C.name);_markNative(C);_graphicsDefineGlobal(C.name,C);}

for(const name of Object.keys(_WEBGPU_CONSTANTS)){const o={};for(const k of Object.keys(_WEBGPU_CONSTANTS[name]))Object.defineProperty(o,k,{value:_WEBGPU_CONSTANTS[name][k],enumerable:true});_graphicsDefineGlobal(name,Object.freeze(o));}

function _gpuSecureContext(){let url='about:blank';try{url=__currentUrl();}catch(_){}try{const u=new URL(url);if(u.protocol==='https:'||u.protocol==='wss:'||u.protocol==='file:'||u.protocol==='about:')return true;if(u.protocol==='http:'&&(u.hostname==='localhost'||u.hostname==='127.0.0.1'||u.hostname==='[::1]'))return true;}catch(_){}return false;}
function _gpuProfile(){return _fingerprintProfile&&_fingerprintProfile.graphics&&_fingerprintProfile.graphics.webgpu||{adapters:{}};}
function _gpuError(device,message){const d=_gpuBrand(device,'GPUDevice'),err=new GPUValidationError(message);if(d.errorScopes.length)d.errorScopes[d.errorScopes.length-1].errors.push(err);else{try{device.dispatchEvent&&device.dispatchEvent(new Event('uncapturederror'));}catch(_){}}return err;}
function _gpuOwns(device,object,kind){const s=_gpuSlots.get(object);if(!s||s.kind!==kind||s.device!==device||s.destroyed){_gpuError(device,'The resource is invalid or belongs to another device.');return null;}return s;}
function _gpuSize3D(size){if(Array.isArray(size))return {width:_graphicsUint(size[0],1),height:_graphicsUint(size[1],1),depthOrArrayLayers:_graphicsUint(size[2],1)};size=size||{};return {width:_graphicsUint(size.width,1),height:_graphicsUint(size.height,1),depthOrArrayLayers:_graphicsUint(size.depthOrArrayLayers,1)};}
function _gpuAdapterEntry(options){const adapters=_gpuProfile().adapters||{},o=options||{};if(o.forceFallbackAdapter){for(const k of Object.keys(adapters))if(adapters[k]&&adapters[k].info&&adapters[k].info.isFallbackAdapter)return adapters[k];return null;}const pref=o.powerPreference==='low-power'?'lowPower':o.powerPreference==='high-performance'?'highPerformance':'default';return adapters[pref]||null;}

_graphicsDefineMethod(GPU.prototype,'requestAdapter',0,function(options){const entry=_gpuAdapterEntry(options);if(!entry)return Promise.resolve(null);const adapter=new GPUAdapter(_graphicsObjectToken,{kind:'GPUAdapter',entry});const s=_gpuSlots.get(adapter);s.features=new GPUSupportedFeatures(_graphicsObjectToken,entry.features);s.limits=new GPUSupportedLimits(_graphicsObjectToken,entry.limits);s.info=new GPUAdapterInfo(_graphicsObjectToken,{kind:'GPUAdapterInfo'});Object.assign(s.info,entry.info||{});return Promise.resolve(adapter);});
_graphicsDefineMethod(GPU.prototype,'getPreferredCanvasFormat',0,function(){return _fingerprintProfile&&_fingerprintProfile.graphics&&_fingerprintProfile.graphics.preferredCanvasFormat||'bgra8unorm';});
_graphicsDefineProperties(GPU.prototype,{wgslLanguageFeatures:{get:function(){const s=_gpuBrand(this,'GPU');if(!s.wgsl)s.wgsl=new WGSLLanguageFeatures(_graphicsObjectToken,(_fingerprintProfile&&_fingerprintProfile.graphics&&_fingerprintProfile.graphics.wgslLanguageFeatures)||[]);return s.wgsl;},enumerable:true,configurable:true}});

_graphicsDefineProperties(GPUAdapter.prototype,{
  features:{get:function(){return _gpuBrand(this,'GPUAdapter').features;},enumerable:true,configurable:true},
  limits:{get:function(){return _gpuBrand(this,'GPUAdapter').limits;},enumerable:true,configurable:true},
  info:{get:function(){return _gpuBrand(this,'GPUAdapter').info;},enumerable:true,configurable:true},
  isFallbackAdapter:{get:function(){return !!(_gpuBrand(this,'GPUAdapter').entry.info||{}).isFallbackAdapter;},enumerable:true,configurable:true},
});
_graphicsDefineMethod(GPUAdapter.prototype,'requestDevice',0,function(descriptor){const a=_gpuBrand(this,'GPUAdapter'),d=descriptor||{},required=Array.from(d.requiredFeatures||[],String);for(const feature of required)if(!a.features.has(feature))return Promise.reject(new DOMException("Unsupported required feature: "+feature,'OperationError'));const requested=d.requiredLimits||{},limits=Object.assign({},a.entry.defaultDeviceLimits||{});for(const name of Object.keys(requested)){if(!Object.prototype.hasOwnProperty.call(a.entry.limits,name))return Promise.reject(new DOMException("Unknown required limit: "+name,'OperationError'));const value=Number(requested[name]),available=Number(a.entry.limits[name]);if(!Number.isFinite(value)||value<0)return Promise.reject(new TypeError('A required limit must be a non-negative number.'));const isMin=name.startsWith('min');if((isMin&&value<available)||(!isMin&&value>available))return Promise.reject(new DOMException("Required limit is not supported: "+name,'OperationError'));limits[name]=isMin?Math.min(limits[name]??available,value):Math.max(limits[name]??0,value);}return Promise.resolve(_newGPUDevice(required,limits));});

function _newGPUDevice(features,limits){let resolveLost;const lost=new Promise(r=>resolveLost=r);const device=new GPUDevice(_graphicsObjectToken,{kind:'GPUDevice',features:new GPUSupportedFeatures(_graphicsObjectToken,features),limits:new GPUSupportedLimits(_graphicsObjectToken,limits),errorScopes:[],destroyed:false,lost,resolveLost,serial:0,listeners:new Map()});const queue=new GPUQueue(_graphicsObjectToken,{kind:'GPUQueue',device,serial:0});const d=_gpuSlots.get(device);d.queue=queue;return device;}
_graphicsDefineProperties(GPUDevice.prototype,{
  features:{get:function(){return _gpuBrand(this,'GPUDevice').features;},enumerable:true,configurable:true},limits:{get:function(){return _gpuBrand(this,'GPUDevice').limits;},enumerable:true,configurable:true},queue:{get:function(){return _gpuBrand(this,'GPUDevice').queue;},enumerable:true,configurable:true},lost:{get:function(){return _gpuBrand(this,'GPUDevice').lost;},enumerable:true,configurable:true},
});
_graphicsDefineMethod(GPUDevice.prototype,'destroy',0,function(){const d=_gpuBrand(this,'GPUDevice');if(d.destroyed)return;d.destroyed=true;d.resolveLost(new GPUDeviceLostInfo(_graphicsObjectToken,'destroyed','The device was destroyed.'));});
_graphicsDefineMethod(GPUDevice.prototype,'pushErrorScope',1,function(filter){const d=_gpuBrand(this,'GPUDevice');if(d.errorScopes.length<_GRAPHICS_ERROR_LIMIT)d.errorScopes.push({filter:String(filter),errors:[]});});
_graphicsDefineMethod(GPUDevice.prototype,'popErrorScope',0,function(){const d=_gpuBrand(this,'GPUDevice'),scope=d.errorScopes.pop();if(!scope)return Promise.reject(new DOMException('There is no error scope to pop.','OperationError'));return Promise.resolve(scope.errors[0]||null);});
_graphicsDefineMethod(GPUDevice.prototype,'addEventListener',2,function(type,listener){const d=_gpuBrand(this,'GPUDevice'),key=String(type);if(typeof listener!=='function')return;if(!d.listeners.has(key))d.listeners.set(key,[]);if(!d.listeners.get(key).includes(listener))d.listeners.get(key).push(listener);});
_graphicsDefineMethod(GPUDevice.prototype,'removeEventListener',2,function(type,listener){const d=_gpuBrand(this,'GPUDevice'),list=d.listeners.get(String(type));if(list)d.listeners.set(String(type),list.filter(v=>v!==listener));});
_graphicsDefineMethod(GPUDevice.prototype,'dispatchEvent',1,function(event){const d=_gpuBrand(this,'GPUDevice'),list=d.listeners.get(String(event&&event.type))||[];for(const listener of list.slice())try{listener.call(this,event);}catch(_){}return !(event&&event.defaultPrevented);});

function _gpuCreate(device,C,kind,state){const d=_gpuBrand(device,'GPUDevice');if(d.destroyed){_gpuError(device,'The device is lost.');return new C(_graphicsObjectToken,Object.assign({kind,device,destroyed:true},state||{}));}return new C(_graphicsObjectToken,Object.assign({kind,device},state||{}));}
_graphicsDefineMethod(GPUDevice.prototype,'createBuffer',1,function(desc){desc=desc||{};const size=Number(desc.size),usage=Number(desc.usage);if(!Number.isSafeInteger(size)||size<0||size>Number(this.limits.maxBufferSize||0)){_gpuError(this,'Buffer size is invalid.');return _gpuCreate(this,GPUBuffer,'GPUBuffer',{size:0,usage,destroyed:true});}const exact=size<=_WEBGL_SHADOW_LIMIT?new Uint8Array(size):null;const buffer=_gpuCreate(this,GPUBuffer,'GPUBuffer',{size,usage,bytes:exact,mapState:desc.mappedAtCreation?'mapped':'unmapped',mapMode:desc.mappedAtCreation?2:0,mapOffset:0,mapSize:size,mappedRanges:[]});return buffer;});
_graphicsDefineProperties(GPUBuffer.prototype,{size:{get:function(){return _gpuBrand(this,'GPUBuffer').size;},enumerable:true,configurable:true},usage:{get:function(){return _gpuBrand(this,'GPUBuffer').usage;},enumerable:true,configurable:true},mapState:{get:function(){return _gpuBrand(this,'GPUBuffer').mapState;},enumerable:true,configurable:true}});
_graphicsDefineMethod(GPUBuffer.prototype,'mapAsync',1,function(mode,offset,size){const b=_gpuBrand(this,'GPUBuffer');offset=Number(offset)||0;size=size===undefined?b.size-offset:Number(size);if(b.destroyed||b.mapState!=='unmapped'||offset<0||size<0||offset+size>b.size||!b.bytes)return Promise.reject(new DOMException('Buffer mapping validation failed.','OperationError'));b.mapState='pending';return Promise.resolve().then(()=>{b.mapState='mapped';b.mapMode=Number(mode);b.mapOffset=offset;b.mapSize=size;});});
_graphicsDefineMethod(GPUBuffer.prototype,'mapSync',1,function(mode,offset,size){const b=_gpuBrand(this,'GPUBuffer');offset=Number(offset)||0;size=size===undefined?b.size-offset:Number(size);if(b.destroyed||b.mapState!=='unmapped'||!b.bytes||offset<0||size<0||offset+size>b.size)throw new DOMException('Buffer mapping validation failed.','OperationError');b.mapState='mapped';b.mapMode=Number(mode);b.mapOffset=offset;b.mapSize=size;});
_graphicsDefineMethod(GPUBuffer.prototype,'getMappedRange',0,function(offset,size){const b=_gpuBrand(this,'GPUBuffer');offset=Number(offset)||0;size=size===undefined?b.mapSize-offset:Number(size);if(b.mapState!=='mapped'||!b.bytes||offset<0||size<0||offset+size>b.mapSize)throw new DOMException('Buffer is not mapped for this range.','OperationError');const copy=b.bytes.slice(b.mapOffset+offset,b.mapOffset+offset+size).buffer;b.mappedRanges.push({buffer:copy,offset:b.mapOffset+offset,size});return copy;});
_graphicsDefineMethod(GPUBuffer.prototype,'unmap',0,function(){const b=_gpuBrand(this,'GPUBuffer');if(b.mapState==='mapped'&&b.bytes)for(const r of b.mappedRanges)b.bytes.set(new Uint8Array(r.buffer),r.offset);b.mappedRanges=[];b.mapState='unmapped';});
_graphicsDefineMethod(GPUBuffer.prototype,'destroy',0,function(){const b=_gpuBrand(this,'GPUBuffer');b.destroyed=true;b.bytes=null;b.mapState='unmapped';});

_graphicsDefineMethod(GPUQueue.prototype,'writeBuffer',3,function(buffer,bufferOffset,data,dataOffset,size){const q=_gpuBrand(this,'GPUQueue'),b=_gpuOwns(q.device,buffer,'GPUBuffer'),bytes=_graphicsBytes(data);bufferOffset=Number(bufferOffset)||0;dataOffset=Number(dataOffset)||0;if(!b||!bytes)return;size=size===undefined?bytes.byteLength-dataOffset:Number(size);if(bufferOffset<0||dataOffset<0||size<0||dataOffset+size>bytes.byteLength||bufferOffset+size>b.size){_gpuError(q.device,'writeBuffer range is invalid.');return;}if(b.bytes)b.bytes.set(bytes.subarray(dataOffset,dataOffset+size),bufferOffset);b.digest=_graphicsDigest([b.digest,_graphicsHashBytes(bytes.subarray(dataOffset,dataOffset+size)),bufferOffset]);});
_graphicsDefineMethod(GPUQueue.prototype,'writeTexture',4,function(destination,data,layout,size){const q=_gpuBrand(this,'GPUQueue'),t=_gpuOwns(q.device,destination&&destination.texture,'GPUTexture'),bytes=_graphicsBytes(data),extent=_gpuSize3D(size),l=layout||{};if(!t||!bytes)return;const offset=Number(l.offset)||0,bytesPerRow=Number(l.bytesPerRow)||extent.width*4,need=offset+(extent.height-1)*bytesPerRow+extent.width*4;if(offset<0||bytesPerRow<extent.width*4||need>bytes.byteLength||extent.width>t.size.width||extent.height>t.size.height){_gpuError(q.device,'writeTexture range is invalid.');return;}_surfaceRegion(t.surface,{kind:'pixels',x:Number(destination.origin&&destination.origin.x)||0,y:Number(destination.origin&&destination.origin.y)||0,w:extent.width,h:extent.height,bytes:bytes.slice(offset,need),bytesPerRow,bgra:t.format.startsWith('bgra'),mask:[true,true,true,true]});});
_graphicsDefineMethod(GPUQueue.prototype,'submit',1,function(buffers){const q=_gpuBrand(this,'GPUQueue');for(const commandBuffer of Array.from(buffers||[])){const c=_gpuOwns(q.device,commandBuffer,'GPUCommandBuffer');if(!c)continue;if(c.submitted){_gpuError(q.device,'A command buffer can only be submitted once.');continue;}c.submitted=true;for(const command of c.commands)_gpuRunCommand(q.device,command);for(const texture of c.presentTextures){const t=_gpuSlots.get(texture);if(t&&t.canvasContext){const cs=_gpuCanvasSlots.get(t.canvasContext);if(cs&&cs.currentTexture===texture)cs.currentTexture=null;}}}q.serial++;});
_graphicsDefineMethod(GPUQueue.prototype,'onSubmittedWorkDone',0,function(){return Promise.resolve();});

const _gpuCoreFormats=new Set(['r8unorm','r8snorm','r8uint','r8sint','r16uint','r16sint','r16float','rg8unorm','rg8snorm','rg8uint','rg8sint','r32uint','r32sint','r32float','rg16uint','rg16sint','rg16float','rgba8unorm','rgba8unorm-srgb','rgba8snorm','rgba8uint','rgba8sint','bgra8unorm','bgra8unorm-srgb','rgb10a2uint','rgb10a2unorm','rg11b10ufloat','rgb9e5ufloat','rg32uint','rg32sint','rg32float','rgba16uint','rgba16sint','rgba16float','rgba32uint','rgba32sint','rgba32float','stencil8','depth16unorm','depth24plus','depth24plus-stencil8','depth32float','depth32float-stencil8']);
function _gpuTextureFeature(format){if(format.startsWith('bc'))return'texture-compression-bc';if(format.startsWith('etc2')||format.startsWith('eac'))return'texture-compression-etc2';if(format.startsWith('astc'))return'texture-compression-astc';return null;}
_graphicsDefineMethod(GPUDevice.prototype,'createTexture',1,function(desc){desc=desc||{};const size=_gpuSize3D(desc.size),max=Number(this.limits.maxTextureDimension2D||8192),format=String(desc.format||''),feature=_gpuTextureFeature(format),allowed=_gpuCoreFormats.has(format)||(feature&&this.features.has(feature));if(!size.width||!size.height||size.width>max||size.height>max||!allowed){_gpuError(this,'Texture size or format is invalid.');return _gpuCreate(this,GPUTexture,'GPUTexture',{destroyed:true,size});}return _gpuCreate(this,GPUTexture,'GPUTexture',{size,format,usage:Number(desc.usage),dimension:String(desc.dimension||'2d'),mipLevelCount:Number(desc.mipLevelCount||1),sampleCount:Number(desc.sampleCount||1),surface:_newSurface(size.width,size.height),canvasContext:null});});
_graphicsDefineProperties(GPUTexture.prototype,{width:{get:function(){return _gpuBrand(this,'GPUTexture').size.width;},enumerable:true,configurable:true},height:{get:function(){return _gpuBrand(this,'GPUTexture').size.height;},enumerable:true,configurable:true},depthOrArrayLayers:{get:function(){return _gpuBrand(this,'GPUTexture').size.depthOrArrayLayers;},enumerable:true,configurable:true},mipLevelCount:{get:function(){return _gpuBrand(this,'GPUTexture').mipLevelCount;},enumerable:true,configurable:true},sampleCount:{get:function(){return _gpuBrand(this,'GPUTexture').sampleCount;},enumerable:true,configurable:true},dimension:{get:function(){return _gpuBrand(this,'GPUTexture').dimension;},enumerable:true,configurable:true},format:{get:function(){return _gpuBrand(this,'GPUTexture').format;},enumerable:true,configurable:true},usage:{get:function(){return _gpuBrand(this,'GPUTexture').usage;},enumerable:true,configurable:true}});
_graphicsDefineMethod(GPUTexture.prototype,'createView',0,function(desc){const t=_gpuBrand(this,'GPUTexture');return _gpuCreate(t.device,GPUTextureView,'GPUTextureView',{texture:this,descriptor:Object.assign({},desc||{})});});
_graphicsDefineMethod(GPUTexture.prototype,'destroy',0,function(){_gpuBrand(this,'GPUTexture').destroyed=true;});
_graphicsDefineMethod(GPUDevice.prototype,'createSampler',0,function(desc){return _gpuCreate(this,GPUSampler,'GPUSampler',{descriptor:Object.assign({},desc||{})});});
_graphicsDefineMethod(GPUDevice.prototype,'createBindGroupLayout',1,function(desc){return _gpuCreate(this,GPUBindGroupLayout,'GPUBindGroupLayout',{descriptor:desc||{}});});
_graphicsDefineMethod(GPUDevice.prototype,'createBindGroup',1,function(desc){return _gpuCreate(this,GPUBindGroup,'GPUBindGroup',{descriptor:desc||{}});});
_graphicsDefineMethod(GPUDevice.prototype,'createPipelineLayout',1,function(desc){return _gpuCreate(this,GPUPipelineLayout,'GPUPipelineLayout',{descriptor:desc||{}});});
_graphicsDefineMethod(GPUDevice.prototype,'createShaderModule',1,function(desc){const code=String(desc&&desc.code||''),valid=!!code.trim()&&((code.match(/\{/g)||[]).length===(code.match(/\}/g)||[]).length);return _gpuCreate(this,GPUShaderModule,'GPUShaderModule',{code,digest:_graphicsDigest(code),messages:valid?[]:[new GPUCompilationMessage(_graphicsObjectToken,'WGSL source is empty or malformed.','error',1,1,0,Math.max(1,code.length))]});});
_graphicsDefineMethod(GPUShaderModule.prototype,'getCompilationInfo',0,function(){return Promise.resolve(new GPUCompilationInfo(_graphicsObjectToken,_gpuBrand(this,'GPUShaderModule').messages));});
function _gpuPipeline(device,C,kind,desc){return _gpuCreate(device,C,kind,{descriptor:desc||{},layouts:new Map()});}
_graphicsDefineMethod(GPUDevice.prototype,'createRenderPipeline',1,function(d){return _gpuPipeline(this,GPURenderPipeline,'GPURenderPipeline',d);});
_graphicsDefineMethod(GPUDevice.prototype,'createComputePipeline',1,function(d){return _gpuPipeline(this,GPUComputePipeline,'GPUComputePipeline',d);});
_graphicsDefineMethod(GPUDevice.prototype,'createRenderPipelineAsync',1,function(d){return Promise.resolve(_gpuPipeline(this,GPURenderPipeline,'GPURenderPipeline',d));});
_graphicsDefineMethod(GPUDevice.prototype,'createComputePipelineAsync',1,function(d){return Promise.resolve(_gpuPipeline(this,GPUComputePipeline,'GPUComputePipeline',d));});
for(const C of [GPURenderPipeline,GPUComputePipeline])_graphicsDefineMethod(C.prototype,'getBindGroupLayout',1,function(index){const p=_gpuBrand(this);if(!p.layouts.has(Number(index)))p.layouts.set(Number(index),_gpuCreate(p.device,GPUBindGroupLayout,'GPUBindGroupLayout',{descriptor:{}}));return p.layouts.get(Number(index));});
_graphicsDefineMethod(GPUDevice.prototype,'createQuerySet',1,function(desc){return _gpuCreate(this,GPUQuerySet,'GPUQuerySet',{type:String(desc&&desc.type||''),count:Number(desc&&desc.count||0),results:new BigUint64Array(Number(desc&&desc.count||0))});});
_graphicsDefineMethod(GPUQuerySet.prototype,'destroy',0,function(){_gpuBrand(this,'GPUQuerySet').destroyed=true;});

_graphicsDefineMethod(GPUDevice.prototype,'createCommandEncoder',0,function(desc){return _gpuCreate(this,GPUCommandEncoder,'GPUCommandEncoder',{open:true,passOpen:false,commands:[],presentTextures:new Set()});});
function _gpuEncoder(self){const e=_gpuBrand(self,'GPUCommandEncoder');if(!e.open||e.passOpen){_gpuError(e.device,'The command encoder is not open.');return null;}return e;}
_graphicsDefineMethod(GPUCommandEncoder.prototype,'clearBuffer',1,function(buffer,offset,size){const e=_gpuEncoder(this),b=e&&_gpuOwns(e.device,buffer,'GPUBuffer');if(!e||!b)return;offset=Number(offset)||0;size=size===undefined?b.size-offset:Number(size);if(offset<0||size<0||offset+size>b.size){_gpuError(e.device,'clearBuffer range is invalid.');return;}e.commands.push({op:'clearBuffer',buffer,offset,size});});
_graphicsDefineMethod(GPUCommandEncoder.prototype,'copyBufferToBuffer',2,function(source,sourceOffset,destination,destinationOffset,size){const e=_gpuEncoder(this),a=e&&_gpuOwns(e.device,source,'GPUBuffer'),b=e&&_gpuOwns(e.device,destination,'GPUBuffer');if(!e||!a||!b)return;sourceOffset=Number(sourceOffset)||0;destinationOffset=Number(destinationOffset)||0;size=Number(size);if(sourceOffset<0||destinationOffset<0||size<0||sourceOffset+size>a.size||destinationOffset+size>b.size){_gpuError(e.device,'copyBufferToBuffer range is invalid.');return;}e.commands.push({op:'copyBuffer',source,sourceOffset,destination,destinationOffset,size});});
_graphicsDefineMethod(GPUCommandEncoder.prototype,'copyTextureToBuffer',3,function(source,destination,size){const e=_gpuEncoder(this);if(e)e.commands.push({op:'textureToBuffer',source,destination,size:_gpuSize3D(size)});});
_graphicsDefineMethod(GPUCommandEncoder.prototype,'copyBufferToTexture',3,function(source,destination,size){const e=_gpuEncoder(this);if(e)e.commands.push({op:'bufferToTexture',source,destination,size:_gpuSize3D(size)});});
_graphicsDefineMethod(GPUCommandEncoder.prototype,'copyTextureToTexture',3,function(source,destination,size){const e=_gpuEncoder(this);if(e)e.commands.push({op:'textureToTexture',source,destination,size:_gpuSize3D(size)});});
_graphicsDefineMethod(GPUCommandEncoder.prototype,'beginRenderPass',1,function(desc){const e=_gpuEncoder(this);if(!e)return null;e.passOpen=true;return _gpuCreate(e.device,GPURenderPassEncoder,'GPURenderPassEncoder',{encoder:this,descriptor:desc||{},open:true,commands:[]});});
_graphicsDefineMethod(GPUCommandEncoder.prototype,'beginComputePass',0,function(desc){const e=_gpuEncoder(this);if(!e)return null;e.passOpen=true;return _gpuCreate(e.device,GPUComputePassEncoder,'GPUComputePassEncoder',{encoder:this,descriptor:desc||{},open:true,commands:[]});});
_graphicsDefineMethod(GPUCommandEncoder.prototype,'finish',0,function(desc){const e=_gpuBrand(this,'GPUCommandEncoder');if(!e.open||e.passOpen){_gpuError(e.device,'The command encoder cannot finish while a pass is open.');return _gpuCreate(e.device,GPUCommandBuffer,'GPUCommandBuffer',{commands:[],presentTextures:new Set(),submitted:false});}e.open=false;return _gpuCreate(e.device,GPUCommandBuffer,'GPUCommandBuffer',{commands:e.commands.slice(),presentTextures:new Set(e.presentTextures),submitted:false});});

function _gpuPass(self,kind){const p=_gpuBrand(self,kind);if(!p.open){_gpuError(p.device,'The pass is already ended.');return null;}return p;}
function _gpuEndPass(self,kind){const p=_gpuPass(self,kind);if(!p)return;const e=_gpuSlots.get(p.encoder);p.open=false;e.passOpen=false;if(kind==='GPURenderPassEncoder'){e.commands.push({op:'renderPass',descriptor:p.descriptor,commands:p.commands});for(const a of Array.from(p.descriptor.colorAttachments||[])){const v=a&&a.view&&_gpuSlots.get(a.view);const t=v&&_gpuSlots.get(v.texture);if(t&&t.canvasContext)e.presentTextures.add(v.texture);}}else e.commands.push({op:'computePass',commands:p.commands});}
_graphicsDefineMethod(GPURenderPassEncoder.prototype,'end',0,function(){_gpuEndPass(this,'GPURenderPassEncoder');});
_graphicsDefineMethod(GPUComputePassEncoder.prototype,'end',0,function(){_gpuEndPass(this,'GPUComputePassEncoder');});
for(const C of [GPURenderPassEncoder,GPUComputePassEncoder])for(const name of ['setPipeline','setBindGroup','pushDebugGroup','popDebugGroup','insertDebugMarker','setImmediates'])_graphicsDefineMethod(C.prototype,name,name==='setPipeline'?1:name==='setBindGroup'?2:name==='pushDebugGroup'||name==='insertDebugMarker'?1:0,function(){const p=_gpuPass(this,C===GPURenderPassEncoder?'GPURenderPassEncoder':'GPUComputePassEncoder');if(p)p.commands.push({op:name,args:Array.from(arguments).map(_graphicsDigest)});});
for(const name of ['draw','drawIndexed','drawIndirect','drawIndexedIndirect','setVertexBuffer','setIndexBuffer','setViewport','setScissorRect','setBlendConstant','setStencilReference','executeBundles'])_graphicsDefineMethod(GPURenderPassEncoder.prototype,name,(_WEBGPU_INTERFACES.GPURenderPassEncoder[name]??_WEBGPU_INTERFACES.GPURenderEncoderBase[name]??1),function(){const p=_gpuPass(this,'GPURenderPassEncoder');if(p)p.commands.push({op:name,args:Array.from(arguments).map(_graphicsDigest)});});
for(const name of ['dispatchWorkgroups','dispatchWorkgroupsIndirect'])_graphicsDefineMethod(GPUComputePassEncoder.prototype,name,_WEBGPU_INTERFACES.GPUComputePassEncoder[name]||1,function(){const p=_gpuPass(this,'GPUComputePassEncoder');if(p)p.commands.push({op:name,args:Array.from(arguments).map(_graphicsDigest)});});

function _gpuRunCommand(device,c){if(c.op==='clearBuffer'){const b=_gpuSlots.get(c.buffer);if(b&&b.bytes)b.bytes.fill(0,c.offset,c.offset+c.size);if(b)b.digest=_graphicsDigest(['clear',c.offset,c.size]);return;}if(c.op==='copyBuffer'){const a=_gpuSlots.get(c.source),b=_gpuSlots.get(c.destination);if(a&&b&&a.bytes&&b.bytes)b.bytes.set(a.bytes.slice(c.sourceOffset,c.sourceOffset+c.size),c.destinationOffset);if(a&&b)b.digest=_graphicsDigest([a.digest,c.sourceOffset,c.destinationOffset,c.size]);return;}if(c.op==='renderPass'){for(const a of Array.from(c.descriptor.colorAttachments||[])){const v=a&&a.view&&_gpuSlots.get(a.view),t=v&&_gpuSlots.get(v.texture);if(!t||!t.surface)continue;if(a.loadOp==='clear'){const color=a.clearValue||[0,0,0,0],values=Array.isArray(color)?color:[color.r,color.g,color.b,color.a];_surfaceRegion(t.surface,{kind:'clear',x:0,y:0,w:t.surface.width,h:t.surface.height,color:values.map(v=>Math.round(Math.max(0,Math.min(1,Number(v)||0))*255)),mask:[true,true,true,true]});if(t.canvasContext){const cs=_gpuCanvasSlots.get(t.canvasContext),canvas=_canvasSlots.get(cs.canvas);canvas.surface=t.surface;}}}return;}if(c.op==='bufferToTexture'){const b=_gpuSlots.get(c.source&&c.source.buffer),t=_gpuSlots.get(c.destination&&c.destination.texture);if(!b||!b.bytes||!t||!t.surface)return;const bytesPerRow=Number(c.source.bytesPerRow||c.size.width*4),offset=Number(c.source.offset||0);_surfaceRegion(t.surface,{kind:'pixels',x:Number(c.destination.origin&&c.destination.origin.x)||0,y:Number(c.destination.origin&&c.destination.origin.y)||0,w:c.size.width,h:c.size.height,bytes:b.bytes.slice(offset,offset+(c.size.height-1)*bytesPerRow+c.size.width*4),bytesPerRow,bgra:t.format.startsWith('bgra'),mask:[true,true,true,true]});return;}if(c.op==='textureToBuffer'){const t=_gpuSlots.get(c.source&&c.source.texture),b=_gpuSlots.get(c.destination&&c.destination.buffer);if(!t||!b||!b.bytes||!t.surface)return;const bytesPerRow=Number(c.destination.bytesPerRow||c.size.width*4),offset=Number(c.destination.offset||0);for(let y=0;y<c.size.height;y++)for(let x=0;x<c.size.width;x++){const at=offset+y*bytesPerRow+x*4;if(at+4<=b.bytes.length)b.bytes.set(_surfacePixel(t.surface,x,y),at);}return;}if(c.op==='textureToTexture'){const a=_gpuSlots.get(c.source&&c.source.texture),b=_gpuSlots.get(c.destination&&c.destination.texture);if(a&&b&&a.surface&&b.surface){b.surface.base=a.surface.base.slice();b.surface.regions=a.surface.regions.slice();}}}

_graphicsDefineMethod(GPUDevice.prototype,'createRenderBundleEncoder',1,function(desc){return _gpuCreate(this,GPURenderBundleEncoder,'GPURenderBundleEncoder',{descriptor:desc||{},commands:[],open:true});});
for(const name of ['draw','drawIndexed','drawIndirect','drawIndexedIndirect','setVertexBuffer','setIndexBuffer','setPipeline','setBindGroup'])_graphicsDefineMethod(GPURenderBundleEncoder.prototype,name,1,function(){const r=_gpuBrand(this,'GPURenderBundleEncoder');if(r.open)r.commands.push({op:name,args:Array.from(arguments).map(_graphicsDigest)});});
_graphicsDefineMethod(GPURenderBundleEncoder.prototype,'finish',0,function(desc){const r=_gpuBrand(this,'GPURenderBundleEncoder');r.open=false;return _gpuCreate(r.device,GPURenderBundle,'GPURenderBundle',{commands:r.commands.slice()});});

function _gpuCanvasResize(context){const c=_gpuCanvasSlots.get(context);if(!c)return;if(c.currentTexture){_gpuSlots.get(c.currentTexture).destroyed=true;c.currentTexture=null;}}
_graphicsDefineMethod(GPUCanvasContext.prototype,'configure',1,function(config){const c=_gpuCanvasSlots.get(this);if(!c)throw new TypeError('Illegal invocation');config=config||{};const d=_gpuSlots.get(config.device);if(!d||d.kind!=='GPUDevice'||d.destroyed)throw new TypeError('A live GPUDevice is required.');const format=String(config.format||'');if(!['bgra8unorm','rgba8unorm','rgba16float'].includes(format))throw new TypeError('The canvas format is not supported.');c.configuration={device:config.device,format,usage:Number(config.usage||0x10),alphaMode:String(config.alphaMode||'opaque'),colorSpace:String(config.colorSpace||'srgb'),toneMapping:config.toneMapping||{mode:'standard'},viewFormats:Array.from(config.viewFormats||[],String)};_gpuCanvasResize(this);});
_graphicsDefineMethod(GPUCanvasContext.prototype,'getConfiguration',0,function(){const c=_gpuCanvasSlots.get(this);return c&&c.configuration?Object.assign({},c.configuration,{viewFormats:c.configuration.viewFormats.slice()}):null;});
_graphicsDefineMethod(GPUCanvasContext.prototype,'getCurrentTexture',0,function(){const c=_gpuCanvasSlots.get(this);if(!c||!c.configuration)throw new DOMException('The canvas context is not configured.','InvalidStateError');if(c.currentTexture&&!_gpuSlots.get(c.currentTexture).destroyed)return c.currentTexture;const size=_canvasSize(c.canvas),texture=_gpuCreate(c.configuration.device,GPUTexture,'GPUTexture',{size:{width:Math.max(1,size[0]),height:Math.max(1,size[1]),depthOrArrayLayers:1},format:c.configuration.format,usage:c.configuration.usage,dimension:'2d',mipLevelCount:1,sampleCount:1,surface:_newSurface(Math.max(1,size[0]),Math.max(1,size[1])),canvasContext:this});c.currentTexture=texture;return texture;});
_graphicsDefineMethod(GPUCanvasContext.prototype,'unconfigure',0,function(){const c=_gpuCanvasSlots.get(this);if(!c)throw new TypeError('Illegal invocation');_gpuCanvasResize(this);c.configuration=null;});

// Replace the generic slot made by _gpuClass with the canvas-specific one.
const _oldCanvasGetContext=_canvasGetContext;
_canvasGetContext=function(canvas,type,options){const value=_oldCanvasGetContext(canvas,type,options);if(value&&String(type).toLowerCase()==='webgpu'&&!_gpuCanvasSlots.has(value))_gpuCanvasSlots.set(value,{canvas,configuration:null,currentTexture:null});return value;};

const _gpuSingleton=new GPU(_graphicsObjectToken,{kind:'GPU',wgsl:null});
_graphicsDefineProperties(Navigator.prototype,{gpu:{get:function(){if(!_navigatorInstances.has(this))throw new TypeError('Illegal invocation');return _gpuSecureContext()&&_gpuProfile().adapters&&Object.keys(_gpuProfile().adapters).length?_gpuSingleton:undefined;},enumerable:true,configurable:true}});

// Fill any still-unimplemented Chrome 145 WebGPU method with a branded,
// bounded logical no-op. Concrete work above keeps its own implementation.
for(const interfaceName of Object.keys(_WEBGPU_INTERFACES)){
  const C=globalThis[interfaceName],methods=_WEBGPU_INTERFACES[interfaceName];if(typeof C!=='function')continue;
  for(const name of Object.keys(methods))if(typeof C.prototype[name]!=='function')_graphicsDefineMethod(C.prototype,name,methods[name],function(){_gpuBrand(this,interfaceName);return undefined;});
}
