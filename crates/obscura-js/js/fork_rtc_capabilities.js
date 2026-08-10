// Fork-only. Chrome 151 Windows RTP capability tables measured with raw CDP.
// Ozon's challenge reads RTCRtpSender.getCapabilities for both media kinds;
// an absent constructor removes the full codec/header-extension rows from its
// browser descriptor even though RTCPeerConnection itself exists upstream.
(function _forkRtcCapabilities() {
  const audioCodecs = [
    { channels: 2, clockRate: 48000, mimeType: 'audio/opus', sdpFmtpLine: 'minptime=10;useinbandfec=1' },
    { channels: 2, clockRate: 48000, mimeType: 'audio/red' },
    { channels: 1, clockRate: 8000, mimeType: 'audio/G722' },
    { channels: 1, clockRate: 8000, mimeType: 'audio/PCMU' },
    { channels: 1, clockRate: 8000, mimeType: 'audio/PCMA' },
    { channels: 1, clockRate: 8000, mimeType: 'audio/CN' },
    { channels: 1, clockRate: 48000, mimeType: 'audio/telephone-event' },
    { channels: 1, clockRate: 8000, mimeType: 'audio/telephone-event' },
  ];
  const senderVideoCodecs = [
    { clockRate: 90000, mimeType: 'video/VP8' },
    { clockRate: 90000, mimeType: 'video/rtx' },
    { clockRate: 90000, mimeType: 'video/H264', sdpFmtpLine: 'level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42001f' },
    { clockRate: 90000, mimeType: 'video/H264', sdpFmtpLine: 'level-asymmetry-allowed=1;packetization-mode=0;profile-level-id=42001f' },
    { clockRate: 90000, mimeType: 'video/H264', sdpFmtpLine: 'level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f' },
    { clockRate: 90000, mimeType: 'video/H264', sdpFmtpLine: 'level-asymmetry-allowed=1;packetization-mode=0;profile-level-id=42e01f' },
    { clockRate: 90000, mimeType: 'video/H264', sdpFmtpLine: 'level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=4d001f' },
    { clockRate: 90000, mimeType: 'video/H264', sdpFmtpLine: 'level-asymmetry-allowed=1;packetization-mode=0;profile-level-id=4d001f' },
    { clockRate: 90000, mimeType: 'video/AV1', sdpFmtpLine: 'level-idx=5;profile=0;tier=0' },
    { clockRate: 90000, mimeType: 'video/VP9', sdpFmtpLine: 'profile-id=0' },
    { clockRate: 90000, mimeType: 'video/VP9', sdpFmtpLine: 'profile-id=2' },
    { clockRate: 90000, mimeType: 'video/H264', sdpFmtpLine: 'level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=640032' },
    { clockRate: 90000, mimeType: 'video/H265', sdpFmtpLine: 'level-id=123;profile-id=1;tier-flag=0;tx-mode=SRST' },
    { clockRate: 90000, mimeType: 'video/red' },
    { clockRate: 90000, mimeType: 'video/ulpfec' },
  ];
  const receiverVideoCodecs = [
    { clockRate: 90000, mimeType: 'video/VP8' },
    { clockRate: 90000, mimeType: 'video/rtx' },
    { clockRate: 90000, mimeType: 'video/VP9', sdpFmtpLine: 'profile-id=0' },
    { clockRate: 90000, mimeType: 'video/VP9', sdpFmtpLine: 'profile-id=2' },
    { clockRate: 90000, mimeType: 'video/VP9', sdpFmtpLine: 'profile-id=1' },
    { clockRate: 90000, mimeType: 'video/VP9', sdpFmtpLine: 'profile-id=3' },
    { clockRate: 90000, mimeType: 'video/H264', sdpFmtpLine: 'level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42001f' },
    { clockRate: 90000, mimeType: 'video/H264', sdpFmtpLine: 'level-asymmetry-allowed=1;packetization-mode=0;profile-level-id=42001f' },
    { clockRate: 90000, mimeType: 'video/H264', sdpFmtpLine: 'level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f' },
    { clockRate: 90000, mimeType: 'video/H264', sdpFmtpLine: 'level-asymmetry-allowed=1;packetization-mode=0;profile-level-id=42e01f' },
    { clockRate: 90000, mimeType: 'video/H264', sdpFmtpLine: 'level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=4d001f' },
    { clockRate: 90000, mimeType: 'video/H264', sdpFmtpLine: 'level-asymmetry-allowed=1;packetization-mode=0;profile-level-id=4d001f' },
    { clockRate: 90000, mimeType: 'video/H264', sdpFmtpLine: 'level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=f4001f' },
    { clockRate: 90000, mimeType: 'video/H264', sdpFmtpLine: 'level-asymmetry-allowed=1;packetization-mode=0;profile-level-id=f4001f' },
    { clockRate: 90000, mimeType: 'video/AV1', sdpFmtpLine: 'level-idx=5;profile=0;tier=0' },
    { clockRate: 90000, mimeType: 'video/AV1', sdpFmtpLine: 'level-idx=5;profile=1;tier=0' },
    { clockRate: 90000, mimeType: 'video/H264', sdpFmtpLine: 'level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=64001f' },
    { clockRate: 90000, mimeType: 'video/H264', sdpFmtpLine: 'level-asymmetry-allowed=1;packetization-mode=0;profile-level-id=64001f' },
    { clockRate: 90000, mimeType: 'video/H265', sdpFmtpLine: 'level-id=180;profile-id=1;tier-flag=0;tx-mode=SRST' },
    { clockRate: 90000, mimeType: 'video/H265', sdpFmtpLine: 'level-id=180;profile-id=2;tier-flag=0;tx-mode=SRST' },
    { clockRate: 90000, mimeType: 'video/red' },
    { clockRate: 90000, mimeType: 'video/ulpfec' },
    { clockRate: 90000, mimeType: 'video/flexfec-03', sdpFmtpLine: 'repair-window=10000000' },
  ];
  const audioHeaderExtensions = [
    'urn:ietf:params:rtp-hdrext:ssrc-audio-level',
    'http://www.webrtc.org/experiments/rtp-hdrext/abs-send-time',
    'http://www.ietf.org/id/draft-holmer-rmcat-transport-wide-cc-extensions-01',
    'urn:ietf:params:rtp-hdrext:sdes:mid',
  ];
  const videoHeaderExtensions = [
    'urn:ietf:params:rtp-hdrext:toffset',
    'http://www.webrtc.org/experiments/rtp-hdrext/abs-send-time',
    'urn:3gpp:video-orientation',
    'http://www.ietf.org/id/draft-holmer-rmcat-transport-wide-cc-extensions-01',
    'http://www.webrtc.org/experiments/rtp-hdrext/playout-delay',
    'http://www.webrtc.org/experiments/rtp-hdrext/video-content-type',
    'http://www.webrtc.org/experiments/rtp-hdrext/video-timing',
    'http://www.webrtc.org/experiments/rtp-hdrext/color-space',
    'urn:ietf:params:rtp-hdrext:sdes:mid',
    'urn:ietf:params:rtp-hdrext:sdes:rtp-stream-id',
    'urn:ietf:params:rtp-hdrext:sdes:repaired-rtp-stream-id',
  ];

  function cloneCapabilities(codecs, headerExtensions) {
    return {
      codecs: codecs.map(codec => ({ ...codec })),
      headerExtensions: headerExtensions.map(uri => ({ direction: 'sendrecv', uri })),
    };
  }

  function capabilityGetter(videoCodecs) {
    const getCapabilities = kind => {
      if (kind === 'audio') return cloneCapabilities(audioCodecs, audioHeaderExtensions);
      if (kind === 'video') return cloneCapabilities(videoCodecs, videoHeaderExtensions);
      return null;
    };
    return _makeNativeFunction(getCapabilities, 'getCapabilities', 1);
  }

  const RTCRtpSender = function RTCRtpSender() { throw new TypeError('Illegal constructor'); };
  const RTCRtpReceiver = function RTCRtpReceiver() { throw new TypeError('Illegal constructor'); };
  _markNative(RTCRtpSender);
  _markNative(RTCRtpReceiver);
  Object.defineProperty(RTCRtpSender, 'getCapabilities', {
    value: capabilityGetter(senderVideoCodecs), writable: true, configurable: true,
  });
  Object.defineProperty(RTCRtpReceiver, 'getCapabilities', {
    value: capabilityGetter(receiverVideoCodecs), writable: true, configurable: true,
  });
  _graphicsDefineGlobal('RTCRtpSender', RTCRtpSender);
  _graphicsDefineGlobal('RTCRtpReceiver', RTCRtpReceiver);
})();
