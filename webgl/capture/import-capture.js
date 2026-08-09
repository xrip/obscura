'use strict';

const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');

function readJson(file) {
  return JSON.parse(fs.readFileSync(file, 'utf8'));
}

function sameJson(left, right) {
  return JSON.stringify(left) === JSON.stringify(right);
}

function checkCapture(profile, windows) {
  if (!profile || profile.profileVersion !== 'obscura-capture-v1' || !profile.fingerprints) {
    throw new Error('the profile file is not an Obscura browser capture');
  }
  if (!Array.isArray(windows) || windows.length !== 1 || windows[0].total !== 1
      || !Array.isArray(windows[0].window) || windows[0].window.length !== 1) {
    throw new Error('the windows file must contain one observation');
  }

  const fingerprints = profile.fingerprints;
  const graphics = fingerprints.hardware && fingerprints.hardware.gpu;
  const browser = fingerprints.browser;
  if (!graphics || !browser) throw new Error('the profile has no browser or graphics block');
  if (!graphics.unmaskedVendor || !graphics.unmaskedRenderer || !graphics.adapter
      || !graphics.preferredCanvasFormat || !Array.isArray(graphics.wgslLanguageFeatures)
      || !browser.webglContext || !browser.webgl2Context) {
    throw new Error('the profile has incomplete graphics data');
  }
  if (!sameJson(windows[0].screen, fingerprints.hardware.screen)
      || !sameJson(windows[0].window[0], browser.window)) {
    throw new Error('the profile and screen files are not from the same capture');
  }
}

function nextProfilePath(directory, profileBytes) {
  const digest = crypto.createHash('sha256').update(profileBytes).digest('hex').slice(0, 16);
  for (let number = 1; number <= 999999; number += 1) {
    const name = `capture-${digest}-${String(number).padStart(3, '0')}.json`;
    const file = path.join(directory, name);
    if (!fs.existsSync(file)) return file;
  }
  throw new Error('no free capture profile file name');
}

function replaceFiles(files) {
  const changed = [];
  try {
    for (const file of files) {
      const existed = fs.existsSync(file.target);
      if (fs.existsSync(file.backup)) fs.rmSync(file.backup);
      if (existed) fs.renameSync(file.target, file.backup);
      try {
        fs.renameSync(file.next, file.target);
      } catch (error) {
        if (existed && fs.existsSync(file.backup)) fs.renameSync(file.backup, file.target);
        throw error;
      }
      changed.push({ ...file, existed });
    }
  } catch (error) {
    for (const file of changed.reverse()) {
      if (fs.existsSync(file.target)) fs.rmSync(file.target);
      if (file.existed && fs.existsSync(file.backup)) fs.renameSync(file.backup, file.target);
    }
    for (const file of files) {
      if (fs.existsSync(file.next)) fs.rmSync(file.next);
    }
    throw error;
  }
  for (const file of changed) {
    if (fs.existsSync(file.backup)) fs.rmSync(file.backup, { force: true });
  }
}

function main(argv) {
  if (argv.length !== 2) {
    throw new Error('usage: node webgl/capture/import-capture.js <obscura-profile.json> <obscura-windows.json>');
  }
  const [profileInput, windowsInput] = argv.map(file => path.resolve(file));
  const root = process.cwd();
  const profileDirectory = path.join(root, 'webgl', 'profiles');
  const windowsTarget = path.join(root, 'webgl', 'window.json');

  const profile = readJson(profileInput);
  const captureWindows = readJson(windowsInput);
  checkCapture(profile, captureWindows);

  const windows = fs.existsSync(windowsTarget) ? readJson(windowsTarget) : [];
  if (!Array.isArray(windows)) {
    throw new Error('the local window.json source is not an array');
  }
  windows.push(...captureWindows);

  fs.mkdirSync(profileDirectory, { recursive: true });
  const profileBytes = `${JSON.stringify(profile, null, 2)}\n`;
  const profileTarget = nextProfilePath(profileDirectory, profileBytes);
  const files = [
    {
      target: windowsTarget,
      next: `${windowsTarget}.obscura-new`,
      backup: `${windowsTarget}.obscura-backup`,
      value: windows,
    },
  ];

  for (const file of files) fs.writeFileSync(file.next, `${JSON.stringify(file.value, null, 2)}\n`);
  fs.writeFileSync(profileTarget, profileBytes, { flag: 'wx' });
  try {
    replaceFiles(files);
  } catch (error) {
    if (fs.existsSync(profileTarget)) fs.rmSync(profileTarget);
    throw error;
  }

  process.stdout.write(`${JSON.stringify({
    profile: path.relative(root, profileTarget),
    windowRows: windows.length,
  }, null, 2)}\n`);
}

if (require.main === module) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    process.stderr.write(`Error: ${error.message}\n`);
    process.exitCode = 1;
  }
}

module.exports = { checkCapture, main };
