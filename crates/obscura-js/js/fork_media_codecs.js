// Fork-only. Spliced at /* __OBSCURA_FORK_LATE_MODULE__ */.
//
// Upstream's HTMLMediaElement.prototype.canPlayType is `return ''`, so the
// engine claims it can play nothing. Measured against the real Chrome on this
// machine, every common type answers "probably":
//
//   video/mp4; codecs="avc1.42E01E"  -> probably
//   audio/mp4; codecs="mp4a.40.2"    -> probably
//   video/webm; codecs="vp8"         -> probably
//
// A page that probes codec support and is told "nothing" has not found a
// browser. Codec probing is also a fingerprint input in its own right, so the
// answers have to be a real Chrome's rather than merely non-empty.
//
// Chrome's rule, which this follows: a known container with a known codec is
// "probably", a known container with no codecs parameter is "maybe", and
// anything else is "". Chrome for Windows ships the proprietary codecs, which
// is why H.264 and AAC answer positively here.
(function _forkMediaCodecs() {
  if (typeof HTMLMediaElement !== 'function') return;

  const CONTAINERS = new Set([
    'video/mp4', 'video/webm', 'video/ogg', 'video/x-matroska', 'video/mpeg',
    'audio/mp4', 'audio/mpeg', 'audio/ogg', 'audio/webm', 'audio/wav',
    'audio/wave', 'audio/x-wav', 'audio/flac', 'audio/aac', 'audio/x-m4a',
    'application/x-mpegurl', 'application/vnd.apple.mpegurl',
  ]);
  // Prefix match, because a codec string carries a profile suffix such as
  // avc1.42E01E or mp4a.40.2.
  const CODECS = [
    'avc1', 'avc3', 'mp4a', 'opus', 'vorbis', 'vp8', 'vp9', 'vp09', 'av01',
    'theora', 'flac', 'ec-3', 'ac-3', 'hvc1', 'hev1', 'mp3', '1',
  ];

  // Containers that name exactly one codec, so Chrome answers "probably" even
  // with no codecs parameter: audio/mpeg is mp3 and nothing else.
  const SELF_DESCRIBING = new Set([
    'audio/mpeg', 'audio/wav', 'audio/wave', 'audio/x-wav', 'audio/flac',
  ]);

  const canPlayType = function canPlayType(type) {
    // Without a fingerprint profile there is no browser identity to answer for,
    // and upstream's position is right: an engine with no decoder that claims
    // support makes applications take a video path that renders nothing. Their
    // test unsupported_media_capabilities_and_readiness_are_honest builds a
    // runtime with no profile and still passes. Same gate as the WebGL facade.
    if (!_fingerprintProfile) return '';
    if (typeof type !== 'string' || type === '') return '';
    const parts = type.toLowerCase().split(';');
    const container = parts[0].trim();
    if (!CONTAINERS.has(container)) return '';

    const codecsPart = parts.slice(1).join(';');
    const match = /codecs\s*=\s*"?([^"]*)"?/.exec(codecsPart);
    if (!match) return SELF_DESCRIBING.has(container) ? 'probably' : 'maybe';

    const codecs = match[1].split(',').map(c => c.trim()).filter(Boolean);
    if (codecs.length === 0) return 'maybe';
    // Chrome answers "probably" only when it recognises every codec listed.
    return codecs.every(c => CODECS.some(known => c === known || c.startsWith(known + '.')))
      ? 'probably'
      : '';
  };

  _markNative(canPlayType);
  Object.defineProperty(HTMLMediaElement.prototype, 'canPlayType', {
    value: canPlayType, writable: true, enumerable: true, configurable: true,
  });
})();
