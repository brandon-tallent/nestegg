/**
 * Headless cookie refresh for Monarch Money
 * Uses saved browser state to extract fresh cookies for the Rust CLI
 * Supports automated re-login with credentials from environment variables
 */
import { chromium } from 'playwright-extra';
import StealthPlugin from 'puppeteer-extra-plugin-stealth';
import { existsSync, readFileSync, writeFileSync, mkdirSync, unlinkSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';
import { logInfo, logError, logDebug, logWarn, timer } from './logger.js';

const __dirname = dirname(fileURLToPath(import.meta.url));
const STATE_DIR = join(__dirname, '..', 'state');
const STATE_FILE = join(STATE_DIR, 'monarch.json');
const OUTPUT_DIR = process.env.OUTPUT_DIR || join(__dirname, '..', '..', 'output');
const COOKIES_FILE = process.env.COOKIES_PATH || join(OUTPUT_DIR, 'cookies.txt');
const DEVICE_UUID_FILE = process.env.DEVICE_UUID_PATH || join(OUTPUT_DIR, 'device-uuid.txt');
const AUTH_TOKEN_FILE = process.env.AUTH_TOKEN_PATH || join(OUTPUT_DIR, 'auth-token.txt');
// One-time code for Monarch's new-device email verification: set MONARCH_OTP,
// or write the code to OTP_FILE while the refresh is waiting on the MFA page.
const OTP_FILE = process.env.OTP_PATH || join(OUTPUT_DIR, 'otp.txt');
const OTP_TIMEOUT_MS = Number(process.env.OTP_TIMEOUT_MS) || 10 * 60 * 1000;
const OTP_POLL_INTERVAL_MS = 2000;
const MFA_VERIFY_TIMEOUT_MS = 30000;

chromium.use(StealthPlugin());

function isLoginPage(url: string): boolean {
  return url.includes('app.monarch.com/login') ||
         url.includes('app.monarch.com/signup') ||
         url.includes('auth.monarch.com');
}

function isMfaPage(url: string): boolean {
  return url.includes('verify') || url.includes('mfa') || url.includes('challenge');
}

async function waitForOtp(): Promise<string | null> {
  const fromEnv = process.env.MONARCH_OTP?.trim();
  if (fromEnv) {
    return fromEnv;
  }

  logWarn('mfa_waiting_for_code', `Waiting for verification code - write it to ${OTP_FILE}`, {
    path: OTP_FILE,
    timeout_ms: OTP_TIMEOUT_MS
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

async function completeMfa(page: any): Promise<boolean> {
  logWarn('mfa_detected', 'MFA page detected - verification code required', { url: page.url() });
  await page.screenshot({ path: join(OUTPUT_DIR, 'mfa-required.png') }).catch(() => {});

  const code = await waitForOtp();
  if (!code) {
    logError('mfa_no_code', 'No verification code provided before timeout');
    return false;
  }

  // The code form is a row of single-character inputs that auto-advance
  const firstInput = await page.$('input:not([type="hidden"])');
  if (!firstInput) {
    logError('mfa_input_not_found', 'Could not find verification code input');
    return false;
  }
  await firstInput.click();
  await page.keyboard.type(code, { delay: 100 });
  logDebug('mfa_code_entered', 'Entered verification code');

  const verifyButton = await page.$('button:has-text("Verify"):not([disabled])');
  if (verifyButton) {
    await verifyButton.click().catch(() => {});
  }

  try {
    await page.waitForURL((url: URL) => !isLoginPage(url.toString()), { timeout: MFA_VERIFY_TIMEOUT_MS });
  } catch {
    logError('mfa_failed', 'Still on login page after submitting verification code', { url: page.url() });
    await page.screenshot({ path: join(OUTPUT_DIR, 'mfa-failed.png') }).catch(() => {});
    return false;
  }

  logInfo('mfa_success', 'Verification code accepted');
  return true;
}

async function performLogin(page: any): Promise<boolean> {
  const email = process.env.MONARCH_EMAIL;
  const password = process.env.MONARCH_PASSWORD;

  if (!email || !password) {
    logError('login_missing_creds', 'MONARCH_EMAIL and MONARCH_PASSWORD environment variables required');
    return false;
  }

  logInfo('login_start', 'Beginning automated login', { url: page.url() });
  const loginTimer = timer('login');

  try {
    // Wait for page to fully load
    await page.waitForLoadState('networkidle', { timeout: 30000 }).catch(() => {});
    await page.waitForTimeout(3000);

    // Take screenshot for debugging
    await page.screenshot({ path: join(OUTPUT_DIR, 'login-page.png') }).catch(() => {});

    // Monarch uses username/password form (email is in a text field named "username")
    const emailInput = await page.$('input[name="username"], input[type="email"], input[name="email"], input[placeholder*="email" i]');
    if (!emailInput) {
      logError('login_email_not_found', 'Could not find email input field');
      return false;
    }

    await emailInput.fill(email);
    logDebug('login_email_entered', 'Entered email');
    await page.waitForTimeout(500);

    const passwordInput = await page.$('input[type="password"], input[name="password"]');
    if (!passwordInput) {
      logError('login_password_not_found', 'Could not find password input field');
      return false;
    }

    await passwordInput.fill(password);
    logDebug('login_password_entered', 'Entered password');
    await page.waitForTimeout(500);

    // Find and click submit button
    const submitSelectors = [
      'button[type="submit"]',
      'button:has-text("Log in")',
      'button:has-text("Sign in")',
      'button:has-text("Continue")',
      '[data-testid="login-button"]',
    ];

    let submitButton = null;
    for (const selector of submitSelectors) {
      submitButton = await page.$(selector);
      if (submitButton) {
        logDebug('login_submit_found', `Found submit button with selector: ${selector}`);
        break;
      }
    }

    if (submitButton) {
      await submitButton.click();
      logInfo('login_submitted', 'Login form submitted');
    } else {
      logDebug('login_submit_enter', 'No submit button found, pressing Enter');
      await page.keyboard.press('Enter');
    }

    // Wait for navigation
    logInfo('login_waiting', 'Waiting for login to complete');
    await page.waitForTimeout(10000);

    const postLoginUrl = page.url();
    logDebug('login_post_url', 'Post-login URL check', { url: postLoginUrl });

    // New devices are challenged with an emailed verification code
    if (isMfaPage(postLoginUrl)) {
      if (!await completeMfa(page)) {
        return false;
      }
      logInfo('login_success', 'Login successful', { duration_ms: loginTimer() });
      return true;
    }

    if (isLoginPage(postLoginUrl)) {
      logError('login_still_on_page', 'Still on login page after login attempt', {
        url: postLoginUrl,
        duration_ms: loginTimer()
      });
      await page.screenshot({ path: join(OUTPUT_DIR, 'login-failed.png') }).catch(() => {});
      return false;
    }

    logInfo('login_success', 'Login successful', { duration_ms: loginTimer() });
    return true;
  } catch (error) {
    logError('login_exception', `Login failed: ${error}`, {
      duration_ms: loginTimer(),
      error: String(error)
    });
    await page.screenshot({ path: join(OUTPUT_DIR, 'login-error.png') }).catch(() => {});
    return false;
  }
}

async function extractDeviceUuid(page: any): Promise<string | null> {
  try {
    // Try localStorage first
    const uuid = await page.evaluate(() => {
      return localStorage.getItem('device-uuid') ||
             localStorage.getItem('deviceUuid') ||
             localStorage.getItem('monarch-device-uuid');
    });

    if (uuid) {
      logDebug('device_uuid_found', 'Found device UUID in localStorage', { uuid });
      return uuid;
    }
  } catch (e) {
    logDebug('device_uuid_localStorage_error', `Could not read localStorage: ${e}`);
  }

  // Generate a new UUID if none found (Monarch accepts this)
  const newUuid = crypto.randomUUID();
  logInfo('device_uuid_generated', 'Generated new device UUID', { uuid: newUuid });
  return newUuid;
}

async function main() {
  // Ensure directories exist
  if (!existsSync(STATE_DIR)) {
    mkdirSync(STATE_DIR, { recursive: true });
  }
  if (!existsSync(OUTPUT_DIR)) {
    mkdirSync(OUTPUT_DIR, { recursive: true });
  }

  const hasSavedState = existsSync(STATE_FILE);
  logInfo('browser_start', 'Starting Monarch Money session refresh', { has_saved_state: hasSavedState });
  const totalTimer = timer('total');

  const browser = await chromium.launch({
    headless: true,
    channel: process.env.BROWSER_CHANNEL || undefined,
    args: [
      '--disable-blink-features=AutomationControlled',
      '--no-sandbox',
      '--disable-setuid-sandbox',
      '--disable-dev-shm-usage',
    ],
  });

  logDebug('browser_launched', 'Browser launched', { headless: true });

  const contextOptions: any = {
    viewport: { width: 1920, height: 1080 },
    // Report the real browser version: a stale one disagrees with the client hints Chrome sends
    userAgent: `Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/${browser.version().split('.')[0]}.0.0.0 Safari/537.36`,
    locale: 'en-US',
    timezoneId: 'America/Los_Angeles',
  };

  if (hasSavedState) {
    logInfo('state_loading', 'Loading saved browser state');
    contextOptions.storageState = STATE_FILE;
  } else {
    logInfo('state_fresh', 'No saved state found, will perform fresh login');
  }

  const context = await browser.newContext(contextOptions);
  const page = await context.newPage();

  // Set up request interception to capture device-uuid and auth token from GraphQL requests
  let capturedDeviceUuid: string | null = null;
  let capturedAuthToken: string | null = null;
  await page.route('**/*', async (route) => {
    const headers = route.request().headers();
    const url = route.request().url();

    // Capture from GraphQL requests which have the auth token
    if (url.includes('graphql')) {
      if (headers['device-uuid'] && !capturedDeviceUuid) {
        capturedDeviceUuid = headers['device-uuid'];
        logDebug('device_uuid_captured', 'Captured device UUID from request header', { uuid: capturedDeviceUuid });
      }
      if (headers['authorization'] && !capturedAuthToken) {
        capturedAuthToken = headers['authorization'];
        logInfo('auth_token_captured', 'Captured auth token from GraphQL request', {
          tokenPrefix: capturedAuthToken.substring(0, 20) + '...'
        });
      }
    }
    await route.continue();
  });

  logInfo('navigation_start', 'Navigating to Monarch Money');
  const navTimer = timer('navigation');

  // Navigate to dashboard (will redirect to login if needed)
  await page.goto('https://app.monarch.com/dashboard', {
    waitUntil: 'domcontentloaded',
    timeout: 60000,
  });

  await page.waitForTimeout(5000);

  let currentUrl = page.url();
  logInfo('navigation_complete', 'Initial navigation complete', {
    duration_ms: navTimer(),
    url: currentUrl
  });

  if (isLoginPage(currentUrl)) {
    logInfo('login_required', 'Login required, attempting automated login');

    const loginSuccess = await performLogin(page);
    if (!loginSuccess) {
      logError('login_failed', 'Automated login failed - run "npm run login" for manual login');
      await browser.close();
      process.exit(1);
    }

    await page.waitForTimeout(3000);
    currentUrl = page.url();
  }

  // Navigate to accounts page to ensure full session and fresh cookies
  logInfo('accounts_navigation_start', 'Navigating to accounts page');
  const accountsTimer = timer('accounts');

  await page.goto('https://app.monarch.com/accounts', {
    waitUntil: 'domcontentloaded',
    timeout: 60000,
  });
  await page.waitForTimeout(3000);

  currentUrl = page.url();
  logInfo('accounts_navigation_complete', 'Accounts navigation complete', {
    duration_ms: accountsTimer(),
    url: currentUrl
  });

  if (isLoginPage(currentUrl)) {
    logError('auth_failed', 'Still being redirected to login - authentication failed', { url: currentUrl });
    await browser.close();
    process.exit(1);
  }

  logInfo('cookies_extracting', 'Session valid, extracting cookies and device UUID');

  // Extract cookies
  const cookies = await context.cookies();
  const monarchCookies = cookies.filter((c: any) =>
    c.domain.includes('monarch.com') || c.domain.includes('monarch.money')
  );

  logInfo('cookies_extracted', 'Cookies extracted', {
    total_cookies: cookies.length,
    monarch_cookies: monarchCookies.length
  });

  const cookieLines = monarchCookies.map((c: any) => `${c.name}=${c.value}`);
  writeFileSync(COOKIES_FILE, cookieLines.join('\n') + '\n');
  logInfo('cookies_saved', `Cookies exported to: ${COOKIES_FILE}`, { path: COOKIES_FILE });

  // Extract device UUID
  let deviceUuid = capturedDeviceUuid || await extractDeviceUuid(page);
  if (deviceUuid) {
    writeFileSync(DEVICE_UUID_FILE, deviceUuid + '\n');
    logInfo('device_uuid_saved', `Device UUID exported to: ${DEVICE_UUID_FILE}`, {
      path: DEVICE_UUID_FILE,
      uuid: deviceUuid
    });
  }

  // Save auth token if the web app still sends one (legacy; cookies are used otherwise)
  if (capturedAuthToken) {
    writeFileSync(AUTH_TOKEN_FILE, capturedAuthToken + '\n');
    logInfo('auth_token_saved', `Auth token exported to: ${AUTH_TOKEN_FILE}`, {
      path: AUTH_TOKEN_FILE,
      tokenPrefix: capturedAuthToken.substring(0, 20) + '...'
    });
  } else {
    // Expected: Monarch's web app authenticates with the session cookie, not a token
    logDebug('auth_token_absent', 'No Authorization header on GraphQL requests - CLI will use session cookies');
  }

  // Save browser state for next refresh
  await context.storageState({ path: STATE_FILE });
  logInfo('state_saved', 'Browser state saved', { path: STATE_FILE });

  await browser.close();
  logInfo('browser_complete', 'Browser automation complete', {
    duration_ms: totalTimer(),
    success: true
  });
}

main().catch((error) => {
  logError('fatal_error', `Fatal error: ${error}`, { error: String(error) });
  process.exit(1);
});
