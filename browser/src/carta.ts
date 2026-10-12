/**
 * Carta equity capture
 *
 * Logs into Carta, then replays the read-only API routes its own web app uses
 * and writes every response into one bundle file for the Rust CLI to load.
 *
 * Carta's login needs a full browser and so a display: use `npm run carta`,
 * which wraps it in xvfb.
 * Only GET requests are made.
 */
import { chromium } from 'playwright-extra';
import StealthPlugin from 'puppeteer-extra-plugin-stealth';
import { existsSync, readFileSync, writeFileSync, mkdirSync, unlinkSync, chmodSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';
import { logInfo, logError, logDebug, logWarn, timer } from './logger.js';

const __dirname = dirname(fileURLToPath(import.meta.url));
const STATE_DIR = join(__dirname, '..', 'state');
const STATE_FILE = join(STATE_DIR, 'carta.json');
const OUTPUT_DIR = process.env.OUTPUT_DIR || join(__dirname, '..', '..', 'output');
const BUNDLE_FILE = process.env.CARTA_BUNDLE_PATH || join(OUTPUT_DIR, 'carta-bundle.json');
// One-time code for Carta's two-step verification: set CARTA_OTP, or write the
// code to OTP_FILE while the capture is waiting on the verification page.
const OTP_FILE = process.env.CARTA_OTP_PATH || join(OUTPUT_DIR, 'carta-otp.txt');
const OTP_TIMEOUT_MS = Number(process.env.OTP_TIMEOUT_MS) || 10 * 60 * 1000;
const OTP_POLL_INTERVAL_MS = 2000;
// Verification method to switch to instead of Carta's default text message
const MFA_METHOD = process.env.CARTA_MFA_METHOD?.trim() || '';

const APP_ORIGIN = 'https://app.carta.com';
const LOGIN_URL = 'https://login.app.carta.com/credentials/login/';
const PROFILE_ROUTE = '/api/profiles/users/me/';
// The login host's two-factor pages all live under this path
const MFA_PATH_MARKER = '/2fa/';
const MFA_CODE_INPUT = 'input:not([type="checkbox"]):not([type="hidden"]):visible';

const NAVIGATION_TIMEOUT_MS = 60000;
const STEP_TIMEOUT_MS = 30000;
const SETTLE_MS = 3000;
// Carta's app keeps polling, so "network idle" is only a best-effort pause
const IDLE_TIMEOUT_MS = 15000;

// Bundle files hold account data and the state file holds a live session
const PRIVATE_FILE_MODE = 0o600;

// More tender offers than anyone has sold into; one page then holds them all
const TENDER_PAGE_SIZE = 50;

// Security types Carta lists per company; each has its own holdings route
const HOLDING_KINDS = ['options', 'shares', 'rsu', 'rsa', 'warrants', 'convertibles', 'sar', 'piu', 'equity-grants'];

chromium.use(StealthPlugin());

interface Captured {
  status: number;
  body: unknown;
}

interface Bundle {
  captured_at: string;
  portfolio_id: string;
  issuer_ids: string[];
  /** Keyed by request path including query string */
  responses: Record<string, Captured>;
}

function writePrivate(path: string, content: string): void {
  writeFileSync(path, content, { mode: PRIVATE_FILE_MODE });
  chmodSync(path, PRIVATE_FILE_MODE);
}

async function screenshot(page: any, name: string): Promise<void> {
  await page.screenshot({ path: join(OUTPUT_DIR, `carta-${name}.png`) }).catch(() => {});
}

/** GET a same-origin API route from inside the logged-in page */
async function get(page: any, route: string): Promise<Captured> {
  return page.evaluate(async (path: string) => {
    const response = await fetch(path, {
      credentials: 'include',
      headers: { accept: 'application/json, text/plain, */*', 'x-requested-with': 'XMLHttpRequest' },
    });
    const text = await response.text();
    let body: unknown = null;
    try {
      body = JSON.parse(text);
    } catch {
      // Non-JSON bodies (error pages) are not worth keeping
    }
    return { status: response.status, body };
  }, route);
}

async function isAuthenticated(page: any): Promise<boolean> {
  if (!page.url().startsWith(APP_ORIGIN)) {
    return false;
  }
  try {
    const profile = await get(page, PROFILE_ROUTE);
    return profile.status === 200 && profile.body !== null;
  } catch {
    return false;
  }
}

async function waitForOtp(): Promise<string | null> {
  const fromEnv = process.env.CARTA_OTP?.trim();
  if (fromEnv) {
    return fromEnv;
  }

  logWarn('mfa_waiting_for_code', `Waiting for verification code - write it to ${OTP_FILE}`, {
    path: OTP_FILE,
    timeout_ms: OTP_TIMEOUT_MS,
  });

  const deadline = Date.now() + OTP_TIMEOUT_MS;
  while (Date.now() < deadline) {
    if (existsSync(OTP_FILE)) {
      const code = readFileSync(OTP_FILE, 'utf8').trim();
      unlinkSync(OTP_FILE);
      if (code) {
        return code;
      }
    }
    await new Promise((resolve) => setTimeout(resolve, OTP_POLL_INTERVAL_MS));
  }
  return null;
}

/** Wait until the browser has left the login host for the app itself */
async function waitForApp(page: any, timeoutMs: number): Promise<boolean> {
  try {
    await page.waitForURL((url: URL) => url.origin === APP_ORIGIN && !url.pathname.startsWith('/accounts/login'), {
      timeout: timeoutMs,
    });
    return true;
  } catch {
    return false;
  }
}

/**
 * Switch the verification challenge away from Carta's default (a text
 * message) to another method it offers, matched by its label, e.g. "email"
 */
async function chooseMfaMethod(page: any, method: string): Promise<void> {
  const other = await page.$('a:has-text("try other options"), button:has-text("try other options")');
  if (!other) {
    logWarn('mfa_no_other_options', 'Carta offered no alternative verification methods');
    return;
  }
  await other.click();
  await page.waitForLoadState('domcontentloaded').catch(() => {});
  await page.waitForTimeout(SETTLE_MS);
  await screenshot(page, 'mfa-options');

  const choice = page
    .locator('a, button, label, [role="button"], [role="radio"]')
    .filter({ hasText: new RegExp(method, 'i') })
    .first();
  if ((await choice.count()) === 0) {
    logWarn('mfa_method_unavailable', `No verification option matching "${method}"`, { url: page.url().split('?')[0] });
    return;
  }
  await choice.click();
  await page.waitForTimeout(SETTLE_MS);
  await screenshot(page, 'mfa-method-chosen');
  logInfo('mfa_method_chosen', `Requested verification by ${method}`, { url: page.url().split('?')[0] });
}

async function performLogin(page: any): Promise<boolean> {
  const email = process.env.CARTA_EMAIL;
  const password = process.env.CARTA_PASSWORD;
  if (!email || !password) {
    logError('login_missing_creds', 'CARTA_EMAIL and CARTA_PASSWORD environment variables required');
    return false;
  }

  logInfo('login_start', 'Beginning Carta login');
  const loginTimer = timer('login');

  await page.goto(LOGIN_URL, { waitUntil: 'domcontentloaded', timeout: NAVIGATION_TIMEOUT_MS });
  await page.waitForLoadState('networkidle', { timeout: IDLE_TIMEOUT_MS }).catch(() => {});

  // Step 1: email, then "Next" reveals the password field
  const emailInput = await page.waitForSelector('input[name="username"], input[type="email"]', {
    timeout: STEP_TIMEOUT_MS,
  });
  await emailInput.fill(email);
  await screenshot(page, 'login-email');
  const next = await page.$('#email-next-btn, button[name="email-next-btn"]');
  if (next) {
    await next.click();
  } else {
    await page.keyboard.press('Enter');
  }
  logDebug('login_email_entered', 'Submitted email');

  // Step 2: password
  let passwordInput;
  try {
    passwordInput = await page.waitForSelector('input[type="password"]:visible', { timeout: STEP_TIMEOUT_MS });
  } catch {
    // An account set up for single sign-on is sent elsewhere instead
    logError('login_password_not_found', 'No password field after submitting email', { url: page.url() });
    await screenshot(page, 'login-no-password');
    return false;
  }
  await passwordInput.fill(password);
  await screenshot(page, 'login-password');
  await page.keyboard.press('Enter');
  logInfo('login_submitted', 'Submitted password');

  // Step 3: either straight into the app, or a texted verification code first
  const onChallenge = page
    .waitForURL((url: URL) => url.pathname.includes(MFA_PATH_MARKER), { timeout: STEP_TIMEOUT_MS })
    .then(() => 'code')
    .catch(() => null);
  const inApp = waitForApp(page, STEP_TIMEOUT_MS).then((ok) => (ok ? 'app' : null));
  const first = (await Promise.race([onChallenge, inApp])) ?? (await onChallenge) ?? (await inApp);

  if (first === 'code') {
    logWarn('mfa_detected', 'Verification code required', { url: page.url().split('?')[0] });
    await screenshot(page, 'mfa-required');
    if (MFA_METHOD) {
      await chooseMfaMethod(page, MFA_METHOD);
    }
    const code = await waitForOtp();
    if (!code) {
      logError('mfa_no_code', 'No verification code provided before timeout');
      return false;
    }
    const codeInput = await page.waitForSelector(MFA_CODE_INPUT, { timeout: STEP_TIMEOUT_MS });
    await codeInput.click();
    await page.keyboard.type(code, { delay: 80 });
    // "Trust this device for 30 days" spares the next month of runs a code
    const trust = await page.$('input[type="checkbox"]');
    if (trust && !(await trust.isChecked())) {
      await trust.check({ force: true }).catch(() => {});
    }
    await screenshot(page, 'mfa-entered');
    const submit = await page.$('button:has-text("Continue")');
    if (submit) {
      await submit.click();
    } else {
      await page.keyboard.press('Enter');
    }
    logDebug('mfa_code_entered', 'Submitted verification code');
    if (!(await waitForApp(page, STEP_TIMEOUT_MS))) {
      logError('mfa_failed', 'Still on the login host after submitting the code', { url: page.url().split('?')[0] });
      await screenshot(page, 'mfa-failed');
      return false;
    }
  } else if (first !== 'app') {
    logError('login_stalled', 'Login did not reach the app or a verification prompt', { url: page.url().split('?')[0] });
    await screenshot(page, 'login-stalled');
    return false;
  }

  logInfo('login_success', 'Login successful', { duration_ms: loginTimer() });
  return true;
}

/** Collect every value stored under `key` anywhere in a JSON document */
function collect(value: unknown, key: string, found: Set<string>): void {
  if (Array.isArray(value)) {
    value.forEach((item) => collect(item, key, found));
  } else if (value && typeof value === 'object') {
    for (const [name, child] of Object.entries(value)) {
      if (name === key && (typeof child === 'number' || typeof child === 'string')) {
        found.add(String(child));
      } else {
        collect(child, key, found);
      }
    }
  }
}

async function capture(page: any, portfolioId: string): Promise<Bundle> {
  const bundle: Bundle = {
    captured_at: new Date().toISOString(),
    portfolio_id: portfolioId,
    issuer_ids: [],
    responses: {},
  };
  const fetchInto = async (route: string): Promise<Captured> => {
    const captured = await get(page, route);
    bundle.responses[route] = captured;
    if (captured.status !== 200) {
      logDebug('route_status', `${captured.status} ${route}`);
    }
    return captured;
  };

  await fetchInto(PROFILE_ROUTE);
  const companies = await fetchInto(`/api/investors/portfolio/fund/${portfolioId}/list/`);
  await fetchInto(`/api/investors/portfolio/fund/${portfolioId}/entities/`);
  await fetchInto(`/api/tax-advisory/v1/qsbs/portfolio/${portfolioId}/corporation-status/`);
  await fetchInto(`/api/exercise/v1/corporation/${portfolioId}/exercise-history/`);
  await fetchInto(`/api/exercise/v1/corporation/${portfolioId}/exercise-companies/`);
  // Sales into company tender offers, most recent first
  await fetchInto(`/corporations/${portfolioId}/tender_offers/historical_tender_offers_api/?ordering=&page=1&page_size=${TENDER_PAGE_SIZE}&search=`);

  const issuers = new Set<string>();
  collect(companies.body, 'corporation_id', issuers);
  collect(companies.body, 'issuer_id', issuers);
  bundle.issuer_ids = [...issuers];
  logInfo('issuers_found', 'Companies in portfolio', { count: bundle.issuer_ids.length });

  for (const issuer of bundle.issuer_ids) {
    const holdings = `/api/investors/holdings/portfolio/${portfolioId}/corporation/${issuer}`;
    await fetchInto(`/api/issuers/v1/${issuer}/fair-market-value/portfolio/${portfolioId}/`);
    await fetchInto(`${holdings}/securities/`);
    await fetchInto(`${holdings}/holdings-dashboard/`);
    await fetchInto(`${holdings}/aggregate-vesting/`);
    await fetchInto(`${holdings}/estimated-value/`);
    await fetchInto(`/api/investors/portfolio/fund/${portfolioId}/entity/${issuer}/tabs/`);
    await fetchInto(`/api/exercise/v2/corporations/${portfolioId}/issuers/${issuer}/exercise-requests/`);

    const grants = new Set<string>();
    for (const kind of HOLDING_KINDS) {
      // Canceled securities are kept: exercised and tendered shares end up canceled
      const query = kind === 'shares' ? '?sorted_by=issuable_type&exclude_canceled=False&enable_always_show_security=True' : '';
      const captured = await fetchInto(`${holdings}/${kind}/${query}`);
      if (kind === 'options') {
        collect(captured.body, 'id', grants);
        collect(captured.body, 'pk', grants);
      }
    }
    for (const grant of grants) {
      const vesting = await get(page, `/api/corporations/${issuer}/option_grant/${grant}/vesting-data/`);
      // Ids of nested objects are swept up too; only real grants answer here
      if (vesting.status === 200) {
        bundle.responses[`/api/corporations/${issuer}/option_grant/${grant}/vesting-data/`] = vesting;
      }
    }
  }
  return bundle;
}

async function main() {
  for (const dir of [STATE_DIR, OUTPUT_DIR]) {
    if (!existsSync(dir)) {
      mkdirSync(dir, { recursive: true });
    }
  }

  const hasSavedState = existsSync(STATE_FILE);
  logInfo('browser_start', 'Starting Carta capture', { has_saved_state: hasSavedState });
  const totalTimer = timer('total');

  const browser = await chromium.launch({
    // Carta's login needs a full browser
    headless: false,
    channel: process.env.BROWSER_CHANNEL || undefined,
    args: ['--disable-blink-features=AutomationControlled', '--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage'],
  });
  const context = await browser.newContext({
    viewport: { width: 1400, height: 900 },
    locale: 'en-US',
    timezoneId: 'America/Los_Angeles',
    ...(hasSavedState ? { storageState: STATE_FILE } : {}),
  });
  const page = await context.newPage();

  let exitCode = 0;
  try {
    await page.goto(`${APP_ORIGIN}/`, { waitUntil: 'domcontentloaded', timeout: NAVIGATION_TIMEOUT_MS });
    await page.waitForTimeout(SETTLE_MS);

    if (!(await isAuthenticated(page))) {
      logInfo('login_required', 'No valid session, logging in');
      if (!(await performLogin(page))) {
        throw new Error('Carta login failed');
      }
      await page.waitForTimeout(SETTLE_MS);
    }
    writePrivate(STATE_FILE, JSON.stringify(await context.storageState()));
    logInfo('state_saved', 'Browser state saved', { path: STATE_FILE });

    // The app lands on /investors/individual/<portfolio id>/portfolio/
    let portfolioId = process.env.CARTA_PORTFOLIO_ID || '';
    if (!portfolioId) {
      await page.goto(`${APP_ORIGIN}/`, { waitUntil: 'domcontentloaded', timeout: NAVIGATION_TIMEOUT_MS });
      await page
        .waitForURL((url: URL) => /\/investors\/individual\/\d+\//.test(url.pathname), { timeout: STEP_TIMEOUT_MS })
        .catch(() => {});
      portfolioId = page.url().match(/\/investors\/individual\/(\d+)\//)?.[1] || '';
    }
    if (!portfolioId) {
      await screenshot(page, 'no-portfolio');
      throw new Error(`Could not find a portfolio id from ${page.url()}; set CARTA_PORTFOLIO_ID`);
    }

    const bundle = await capture(page, portfolioId);
    writePrivate(BUNDLE_FILE, JSON.stringify(bundle));
    const statuses = Object.values(bundle.responses).map((r) => r.status);
    logInfo('bundle_saved', `Carta bundle written to: ${BUNDLE_FILE}`, {
      path: BUNDLE_FILE,
      routes: statuses.length,
      ok: statuses.filter((s) => s === 200).length,
      issuers: bundle.issuer_ids.length,
    });
  } catch (error) {
    logError('capture_failed', `Carta capture failed: ${error}`, { error: String(error) });
    await screenshot(page, 'error');
    exitCode = 1;
  }

  await browser.close();
  logInfo('browser_complete', 'Carta capture complete', { duration_ms: totalTimer(), success: exitCode === 0 });
  process.exit(exitCode);
}

main().catch((error) => {
  logError('fatal_error', `Fatal error: ${error}`, { error: String(error) });
  process.exit(1);
});
