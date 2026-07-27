#!/usr/bin/env node
/**
 * record.mjs — generic per-segment screen recorder for video clips.
 *
 * Usage: node record.mjs <clip.json> --config <video.config.json|target-repo-dir> [--out <dir>] [--check]
 *
 * All target-specific knowledge (URLs, ready selectors, interactions, overlay
 * dismissal) lives in the target repo's video.config.json — see README.md for
 * the schema. For each timing_map segment with a configured ui_target, opens
 * the mapped page at 1080x1920, runs its ready steps, then performs its motion
 * preset for (to_s - from_s) + PAD seconds while Playwright records video.
 * Writes <out>/segments/seg{NN}_{ui_target}.webm plus segments.json with
 * per-segment ready offsets so composite.py can trim past page load.
 *
 * Exit codes: 0 ok · 2 bad args / config error
 */
import { chromium } from 'playwright-core';
import fs from 'node:fs';
import path from 'node:path';

const PAD_S = 4; // extra footage per segment so composite can trim
const VP = { width: 1080, height: 1920 };
const BUILTIN_TARGETS = new Set(['end_card']); // rendered in composite, never recorded

// --- args ----------------------------------------------------------------------

function usage() {
  console.log(`Usage: node record.mjs <clip.json> --config <video.config.json|dir> [--out <dir>] [--check]

  <clip.json>   clip definition (schema v1)
  --config      target repo's video.config.json, or a directory containing it
  --out         output directory (default: <engine>/out)
  --check       validate clip + config and print resolved segments; no browser`);
}

function parseArgs(argv) {
  const args = { clip: null, config: null, out: null, check: false };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--help' || a === '-h') { usage(); process.exit(0); }
    else if (a === '--config') args.config = argv[++i];
    else if (a === '--out') args.out = argv[++i];
    else if (a === '--check') args.check = true;
    else if (!a.startsWith('--') && !args.clip) args.clip = a;
    else { console.error(`record: unknown argument: ${a}`); process.exit(2); }
  }
  if (!args.clip) { console.error('record: missing <clip.json>'); usage(); process.exit(2); }
  if (!args.config) { console.error('record: missing --config <video.config.json|dir>'); process.exit(2); }
  return args;
}

function loadJson(p, what) {
  try {
    return JSON.parse(fs.readFileSync(p, 'utf8'));
  } catch (e) {
    console.error(`record: cannot read ${what} at ${p}: ${e.message}`);
    process.exit(2);
  }
}

function resolveConfigPath(p) {
  const abs = path.resolve(p);
  if (fs.existsSync(abs) && fs.statSync(abs).isDirectory()) return path.join(abs, 'video.config.json');
  return abs;
}

// --- config -> target plan -------------------------------------------------------

const DEFAULT_OVERLAYS = {
  hide_css: '',
  dismiss_button_pattern: 'got it|decline|dismiss',
  block_routes: '',
};

function buildPlan(config) {
  const baseUrl = config.base_url;
  if (!baseUrl) { console.error('record: config missing base_url'); process.exit(2); }
  const overlays = { ...DEFAULT_OVERLAYS, ...(config.overlays ?? {}) };
  const targets = {};
  for (const [name, t] of Object.entries(config.ui_targets ?? {})) {
    if (t.builtin) continue; // builtin targets are rendered in composite
    if (!t.path) { console.error(`record: ui_target "${name}" missing path`); process.exit(2); }
    targets[name] = {
      url: () => `${baseUrl}${t.path}`,
      ready: t.ready ?? [],
      motion: t.motion ?? 'dwell',
      dwell_text: t.dwell_text ?? null,
      input_tweak: t.input_tweak ?? null,
      hover_text: t.hover_text ?? null,
      interactions: t.interactions ?? [],
    };
  }
  return { overlays, targets };
}

// --- browser helpers -------------------------------------------------------------

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** Hide overlays / chat widgets / banners that pollute the recording. */
async function dismissOverlays(page, overlays, clickTimeout = 2500) {
  if (overlays.hide_css) {
    await page.addStyleTag({ content: `${overlays.hide_css} { display: none !important; }` }).catch(() => {});
  }
  if (overlays.dismiss_button_pattern) {
    try {
      await page.getByRole('button', { name: new RegExp(overlays.dismiss_button_pattern, 'i') })
        .first().click({ timeout: clickTimeout });
    } catch { /* banner may not exist on every page */ }
  }
}

async function wander(page, moves = 3) {
  for (let i = 0; i < moves; i++) {
    await page.mouse.move(200 + Math.random() * 680, 400 + Math.random() * 1100, { steps: 18 });
    await sleep(150 + Math.random() * 350);
  }
}

async function humanScrollDown(page, pixels) {
  let done = 0;
  while (done < pixels) {
    const step = 60 + Math.random() * 90;
    await page.mouse.wheel(0, step);
    done += step;
    await sleep(45 + Math.random() * 85);
  }
}

/** Scroll down-only until the locator sits comfortably in the viewport. */
async function scrollLocatorIntoView(page, locator, maxRounds = 40) {
  for (let i = 0; i < maxRounds; i++) {
    const box = await locator.first().boundingBox().catch(() => null);
    if (box && box.y > 140 && box.y < 1000) return true;
    await humanScrollDown(page, 320 + Math.random() * 240);
  }
  await locator.first().scrollIntoViewIfNeeded().catch(() => {});
  return false;
}

async function newRecordingPage(browser, overlays, segDir) {
  const context = await browser.newContext({
    viewport: VP,
    deviceScaleFactor: 1,
    recordVideo: { dir: segDir, size: VP },
  });
  if (overlays.block_routes) {
    await context.route(new RegExp(overlays.block_routes), (route) => route.abort());
  }
  const page = await context.newPage();
  return { context, page };
}

// --- ready steps + interactions (config primitives) --------------------------------

async function clickRole(page, spec, timeout = 10000) {
  await page.getByRole(spec.role ?? 'button', { name: spec.name, exact: spec.exact ?? false })
    .first().click({ timeout });
}

async function runSteps(page, steps) {
  for (const step of steps) {
    if (step.wait_text) {
      await page.getByText(step.wait_text, { exact: step.exact ?? false }).first().waitFor({ timeout: 120000 });
    } else if (step.scroll_to) {
      await scrollLocatorIntoView(page, page.locator(step.scroll_to));
    } else if (step.scroll_to_text) {
      await scrollLocatorIntoView(page, page.getByText(step.scroll_to_text));
    } else if (step.click_role) {
      await clickRole(page, step.click_role);
    } else if (step.sleep_ms) {
      await sleep(step.sleep_ms);
    } else {
      console.error(`record: unknown ready step ${JSON.stringify(step)}`);
      process.exit(2);
    }
  }
}

// --- motion presets ---------------------------------------------------------------

const MOTIONS = {
  /** Gentle down-only scrolling + mouse wander (headers auto-hide). */
  async dwell_scroll(page, needS) {
    const end = Date.now() + needS * 1000;
    await wander(page, 2);
    while (Date.now() < end) {
      await humanScrollDown(page, 150 + Math.random() * 150);
      await wander(page, 2);
    }
  },

  /** Slow scroll to `dwell_text`, then dwell there; optional one-input tweak. */
  async slow_scroll(page, needS, target) {
    const end = Date.now() + needS * 1000;
    const dwellLocator = target.dwell_text ? page.getByText(target.dwell_text, { exact: false }).first() : null;
    while (Date.now() < end) {
      const box = dwellLocator ? await dwellLocator.boundingBox().catch(() => null) : null;
      if (box && box.y > 140 && box.y < 900) {
        if (target.input_tweak) {
          try {
            const inp = page.locator(target.input_tweak.selector).nth(target.input_tweak.index ?? 0);
            if (await inp.isVisible({ timeout: 1500 })) {
              await inp.click({ timeout: 1500 });
              await inp.fill(String(target.input_tweak.value));
              await sleep(600);
            }
          } catch { /* input tweak is best-effort */ }
        }
        await wander(page, 2);
        await sleep(700);
      } else {
        await humanScrollDown(page, 220 + Math.random() * 160);
      }
      if (Date.now() < end && Math.random() < 0.3) await wander(page, 1);
    }
  },

  /** Stay put: wander, optional hover, optional timed interactions, wander. */
  async dwell(page, needS, target) {
    const end = Date.now() + needS * 1000;
    await wander(page, 2);
    if (target.hover_text) {
      try {
        const el = page.getByText(target.hover_text, { exact: true }).first();
        const box = await el.boundingBox();
        if (box) await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2, { steps: 15 });
      } catch { /* hover is best-effort */ }
      await sleep(1200);
    }
    if (target.interactions.length && Date.now() < end - 4000) {
      for (const action of target.interactions) {
        try {
          if (action.click_role) await clickRole(page, action.click_role, 3000);
          await wander(page, 1);
          await sleep(action.sleep_after_ms ?? 1000);
        } catch { /* interactions are best-effort */ }
      }
    }
    while (Date.now() < end) {
      await wander(page, 2);
      await sleep(500);
    }
  },
};

// --- main -------------------------------------------------------------------------

const args = parseArgs(process.argv.slice(2));
const clipPath = path.resolve(args.clip);
const configPath = resolveConfigPath(args.config);
const clip = loadJson(clipPath, 'clip definition');
const config = loadJson(configPath, 'video config');
const OUT = path.resolve(args.out ?? path.join(path.dirname(new URL(import.meta.url).pathname), 'out'));
const SEG_DIR = path.join(OUT, 'segments');

const { overlays, targets } = buildPlan(config);

const plan = [];
for (let i = 0; i < clip.timing_map.length; i++) {
  const seg = clip.timing_map[i];
  const isBuiltin = BUILTIN_TARGETS.has(seg.ui_target) || config.ui_targets?.[seg.ui_target]?.builtin;
  const target = targets[seg.ui_target];
  plan.push({ i, seg, isBuiltin, target });
}

if (args.check) {
  for (const { i, seg, isBuiltin, target } of plan) {
    if (isBuiltin) console.log(`[check] seg ${i}: ${seg.ui_target} -> builtin (rendered in composite)`);
    else if (!target) { console.error(`[check] seg ${i}: no ui_target "${seg.ui_target}" in ${configPath}`); process.exit(2); }
    else console.log(`[check] seg ${i}: ${seg.ui_target} -> ${config.base_url}${config.ui_targets[seg.ui_target].path} (motion: ${target.motion})`);
  }
  console.log('[check] ok');
  process.exit(0);
}

for (const { i, seg, isBuiltin, target } of plan) {
  if (isBuiltin || target) continue;
  console.error(`record: no ui_target "${seg.ui_target}" in ${configPath}`);
  process.exit(2);
}

fs.mkdirSync(SEG_DIR, { recursive: true });
const browser = await chromium.launch();
const manifest = [];

for (const { i, seg, isBuiltin, target } of plan) {
  if (isBuiltin) {
    console.log(`[record] seg ${i}: ui_target "${seg.ui_target}" is builtin (skipped, rendered in composite)`);
    continue;
  }
  const needS = seg.to_s - seg.from_s + PAD_S;
  console.log(`[record] seg ${i}: ${seg.ui_target} (${needS}s)`);

  const { context, page } = await newRecordingPage(browser, overlays, SEG_DIR);
  const t0 = Date.now();
  await page.goto(target.url(), { waitUntil: 'domcontentloaded', timeout: 120000 });
  await dismissOverlays(page, overlays);
  await runSteps(page, target.ready);
  await dismissOverlays(page, overlays, 1200);
  const readyOffsetS = Math.max(0, (Date.now() - t0) / 1000 - 0.5);

  const motion = MOTIONS[target.motion];
  if (!motion) {
    console.error(`record: unknown motion preset "${target.motion}" for ${seg.ui_target}`);
    process.exit(2);
  }
  await motion(page, needS, target);

  const video = page.video();
  await context.close();
  const rawPath = await video.path();
  const file = `seg${String(i).padStart(2, '0')}_${seg.ui_target}.webm`;
  fs.renameSync(rawPath, path.join(SEG_DIR, file));
  manifest.push({ index: i, ui_target: seg.ui_target, file, ready_offset_s: readyOffsetS });
  console.log(`[record]   -> ${file} (ready at ${readyOffsetS.toFixed(1)}s)`);
}

fs.writeFileSync(path.join(SEG_DIR, 'segments.json'), JSON.stringify(manifest, null, 2));
await browser.close();
console.log('[record] done');
