/**
 * Interactive login script for Monarch Money
 * Opens a visible browser for manual login with MFA support
 * Saves browser state for future headless cookie extraction
 */
import { chromium } from 'playwright-extra';
import StealthPlugin from 'puppeteer-extra-plugin-stealth';
import { existsSync, mkdirSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const STATE_DIR = join(__dirname, '..', 'state');
const STATE_FILE = join(STATE_DIR, 'monarch.json');

chromium.use(StealthPlugin());

function isLoginPage(url: string): boolean {
  return url.includes('app.monarch.com/login') ||
         url.includes('app.monarch.com/signup') ||
         url.includes('auth.monarch.com');
}

function isDashboard(url: string): boolean {
  return url.includes('app.monarch.com/dashboard') ||
         url.includes('app.monarch.com/accounts') ||
         url.includes('app.monarch.com/transactions') ||
         url.includes('app.monarch.com/cashflow');
}

async function main() {
  // Ensure state directory exists
  if (!existsSync(STATE_DIR)) {
    mkdirSync(STATE_DIR, { recursive: true });
  }

  console.log('Launching browser for Monarch Money login...');
  console.log('');
  console.log('Instructions:');
  console.log('  1. Log in with your credentials (email/password or Google/Apple)');
  console.log('  2. Complete MFA/2FA if prompted');
  console.log('  3. Wait for the dashboard to load');
  console.log('  4. The script will auto-detect when login is complete');
  console.log('');

  const browser = await chromium.launch({
    headless: false,  // Visible browser for manual login
    channel: process.env.BROWSER_CHANNEL || undefined,
    args: [
      '--disable-blink-features=AutomationControlled',
      '--no-sandbox',
      '--disable-setuid-sandbox',
      '--disable-dev-shm-usage',
    ],
  });

  // Create context with realistic viewport and settings
  const context = await browser.newContext({
    viewport: { width: 1920, height: 1080 },
    // Report the real browser version: a stale one disagrees with the client hints Chrome sends
    userAgent: `Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/${browser.version().split('.')[0]}.0.0.0 Safari/537.36`,
    locale: 'en-US',
    timezoneId: 'America/Los_Angeles',
  });

  const page = await context.newPage();

  // Set default timeout to 3 minutes for all operations
  page.setDefaultTimeout(180000);

  // Capture device-uuid from localStorage after page loads
  let deviceUuid: string | null = null;
  page.on('console', async (msg) => {
    if (msg.text().includes('device-uuid')) {
      console.log(`[Browser Console] ${msg.text()}`);
    }
  });

  // Navigate to Monarch Money login
  await page.goto('https://app.monarch.com/login', { timeout: 180000 });

  console.log('Waiting for you to log in...');
  console.log('(The script will auto-detect when login is complete - 3 minute timeout)');

  // Wait until URL shows dashboard or account pages
  await page.waitForFunction(
    () => {
      const url = window.location.href;
      return url.includes('/dashboard') ||
             url.includes('/accounts') ||
             url.includes('/transactions') ||
             url.includes('/cashflow') ||
             url.includes('/planning');
    },
    { timeout: 180000 } // 3 minute timeout
  );

  // Give it a moment to settle and load all data
  await page.waitForTimeout(5000);

  const currentUrl = page.url();
  console.log(`Current URL: ${currentUrl}`);
  console.log('Login detected! Extracting device-uuid and saving browser state...');

  // Try to capture device-uuid from localStorage
  try {
    deviceUuid = await page.evaluate(() => {
      return localStorage.getItem('device-uuid') ||
             localStorage.getItem('deviceUuid') ||
             localStorage.getItem('monarch-device-uuid');
    });

    if (deviceUuid) {
      console.log(`Device UUID found: ${deviceUuid}`);
    } else {
      console.log('Device UUID not found in localStorage, will capture from network requests');
    }
  } catch (e) {
    console.log('Could not read localStorage:', e);
  }

  // Save the browser state (cookies, localStorage, etc.)
  await context.storageState({ path: STATE_FILE });
  console.log(`Browser state saved to: ${STATE_FILE}`);

  // If we found a device UUID, save it separately
  if (deviceUuid) {
    const deviceUuidPath = join(__dirname, '..', '..', 'output', 'device-uuid.txt');
    const outputDir = dirname(deviceUuidPath);
    if (!existsSync(outputDir)) {
      mkdirSync(outputDir, { recursive: true });
    }
    const { writeFileSync } = await import('fs');
    writeFileSync(deviceUuidPath, deviceUuid + '\n');
    console.log(`Device UUID saved to: ${deviceUuidPath}`);
  }

  await browser.close();
  console.log('');
  console.log('Done! You can now run "npm run refresh" to export cookies for the CLI.');
}

main().catch(console.error);
