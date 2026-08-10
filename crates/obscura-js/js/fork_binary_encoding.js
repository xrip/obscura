// Fork-only. Spliced before upstream's `btoa = btoa || ...` fallback.
// Browser btoa reads one Latin-1 byte from each code unit. UTF-8 encoding
// changes binary challenge payloads and does not match Chrome.
globalThis.btoa = (value) => {
  const input = String(value);
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  const bytes = new Uint8Array(input.length);
  for (let i = 0; i < input.length; i++) {
    const code = input.charCodeAt(i);
    if (code > 0xFF) {
      throw new DOMException(
        "The string to be encoded contains characters outside of the Latin1 range.",
        "InvalidCharacterError",
      );
    }
    bytes[i] = code;
  }

  let output = "";
  for (let i = 0; i < bytes.length; i += 3) {
    const a = bytes[i];
    const b = bytes[i + 1] ?? 0;
    const c = bytes[i + 2] ?? 0;
    output += alphabet[a >> 2];
    output += alphabet[((a & 3) << 4) | (b >> 4)];
    output += i + 1 < bytes.length ? alphabet[((b & 15) << 2) | (c >> 6)] : "=";
    output += i + 2 < bytes.length ? alphabet[c & 63] : "=";
  }
  return output;
};
