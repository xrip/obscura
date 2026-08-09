// Fork-only. Spliced into bootstrap.js at /* __OBSCURA_GRAPHICS_PAGE_INIT__ */,
// inside __obscura_init, which runs once per page.
//
// crates/obscura-js/src/graphics.rs sets the global just before init runs. It
// is consumed and deleted here so no page ever sees it.
const _injectedProfile = globalThis.__obscura_fingerprint_profile;
if (_injectedProfile && typeof _injectedProfile === 'object') {
  _fingerprintProfile = _freezeFingerprintProfile(_injectedProfile);
}
delete globalThis.__obscura_fingerprint_profile;
