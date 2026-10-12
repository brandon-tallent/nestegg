/**
 * Scrape Monarch Money data via browser-based API calls
 * Uses the authenticated browser session to make GraphQL requests
 */
import { chromium } from 'playwright-extra';
import StealthPlugin from 'puppeteer-extra-plugin-stealth';
import { existsSync, writeFileSync, mkdirSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';
import { logInfo, logError, logDebug, timer } from './logger.js';

const __dirname = dirname(fileURLToPath(import.meta.url));
const STATE_DIR = join(__dirname, '..', 'state');
const STATE_FILE = join(STATE_DIR, 'monarch.json');
const OUTPUT_DIR = process.env.OUTPUT_DIR || join(__dirname, '..', '..', 'output');

chromium.use(StealthPlugin());

const GRAPHQL_ENDPOINT = 'https://api.monarch.com/graphql';

// GraphQL queries
const GET_ACCOUNTS_QUERY = `
query Web_GetAccountsPage {
  accounts {
    id
    displayName
    syncDisabled
    deactivatedAt
    isHidden
    isAsset
    includeInNetWorth
    currentBalance
    displayBalance
    signedBalance
    type { name display __typename }
    subtype { name display __typename }
    credential {
      id
      updateRequired
      __typename
    }
    isManual
    __typename
  }
}`;

// Query uses the correct Monarch schema structure:
// - filters go on allTransactions
// - pagination goes on results
const GET_TRANSACTIONS_QUERY = `
query Web_GetTransactionsList($offset: Int, $limit: Int, $filters: TransactionFilterInput, $orderBy: TransactionOrdering) {
  allTransactions(filters: $filters) {
    totalCount
    results(offset: $offset, limit: $limit, orderBy: $orderBy) {
      id
      amount
      pending
      date
      hideFromReports
      needsReview
      plaidName
      notes
      isSplitTransaction
      account { id displayName __typename }
      category { id name icon __typename }
      merchant { id name __typename }
      tags { id name color __typename }
      __typename
    }
    __typename
  }
}`;

// Uses the correct Monarch schema structure for portfolio
const GET_HOLDINGS_QUERY = `
query Web_GetPortfolio($portfolioInput: PortfolioInput) {
  portfolio(input: $portfolioInput) {
    performance {
      totalValue
      totalBasis
      totalChangePercent
      totalChangeDollars
      __typename
    }
    aggregateHoldings {
      edges {
        node {
          id
          quantity
          basis
          totalValue
          securityPriceChangeDollars
          securityPriceChangePercent
          security {
            id
            name
            ticker
            currentPrice
            __typename
          }
          __typename
        }
        __typename
      }
      __typename
    }
    __typename
  }
}`;

// Uses the correct Monarch schema structure for aggregate snapshots
const GET_AGGREGATE_SNAPSHOTS_QUERY = `
query Common_GetAggregateSnapshots($filters: AggregateSnapshotFilters) {
  aggregateSnapshots(filters: $filters) {
    date
    balance
    assetsBalance
    liabilitiesBalance
    __typename
  }
}`;

interface GraphQLResponse<T> {
  data?: T;
  errors?: Array<{ message: string }>;
}

async function executeGraphQL<T>(
  page: any,
  operationName: string,
  query: string,
  variables: any = {},
  authToken: string,
  deviceUuid: string
): Promise<T> {
  logDebug('graphql_request', `Executing ${operationName}`, { variables });

  // Make the fetch call from within the page context with the captured auth token
  const result = await page.evaluate(async ({ endpoint, opName, q, vars, auth, uuid }: {
    endpoint: string;
    opName: string;
    q: string;
    vars: any;
    auth: string;
    uuid: string;
  }) => {
    const response = await fetch(endpoint, {
      method: 'POST',
      credentials: 'include',
      headers: {
        'Content-Type': 'application/json',
        'Authorization': auth,
        'device-uuid': uuid,
        'monarch-client': 'monarch-core-web-app-graphql',
        'monarch-client-version': 'v1.0.1068',
        'client-platform': 'web',
      },
      body: JSON.stringify({
        operationName: opName,
        query: q,
        variables: vars
      }),
    });
    return {
      status: response.status,
      json: await response.json()
    };
  }, {
    endpoint: GRAPHQL_ENDPOINT,
    opName: operationName,
    q: query,
    vars: variables,
    auth: authToken,
    uuid: deviceUuid
  });

  logDebug('graphql_response', `Response from ${operationName}`, {
    status: result.status,
    hasData: !!result.json?.data,
    hasErrors: !!result.json?.errors,
  });

  if (result.json?.errors && result.json.errors.length > 0) {
    logError('graphql_errors', 'GraphQL returned errors', {
      errors: result.json.errors,
      preview: JSON.stringify(result.json).substring(0, 1000),
    });
    throw new Error(`GraphQL errors: ${result.json.errors.map((e: any) => e.message).join(', ')}`);
  }

  if (!result.json?.data) {
    throw new Error(`No data in response (status ${result.status}): ${JSON.stringify(result.json).substring(0, 500)}`);
  }

  return result.json.data as T;
}

async function fetchAllTransactions(
  page: any,
  authToken: string,
  deviceUuid: string,
  sinceDate?: string
): Promise<any[]> {
  const allTransactions: any[] = [];
  const limit = 100;
  let offset = 0;

  const filters = sinceDate ? { startDate: sinceDate } : {};

  while (true) {
    const data = await executeGraphQL<any>(
      page,
      'Web_GetTransactionsList',
      GET_TRANSACTIONS_QUERY,
      { offset, limit, filters, orderBy: 'date' },
      authToken,
      deviceUuid
    );

    const transactions = data.allTransactions.results;
    const totalCount = data.allTransactions.totalCount;

    logDebug('transactions_page', `Fetched ${transactions.length} transactions (offset: ${offset}, total: ${totalCount})`);
    allTransactions.push(...transactions);

    if (transactions.length < limit || allTransactions.length >= totalCount) {
      break;
    }
    offset += limit;
  }

  return allTransactions;
}

async function main() {
  const hasSavedState = existsSync(STATE_FILE);
  if (!hasSavedState) {
    logError('no_state', 'No saved browser state. Run "npm run login" first.');
    process.exit(1);
  }

  if (!existsSync(OUTPUT_DIR)) {
    mkdirSync(OUTPUT_DIR, { recursive: true });
  }

  logInfo('scrape_start', 'Starting Monarch Money data scrape');
  const totalTimer = timer('total');

  const browser = await chromium.launch({
    headless: true,
    args: [
      '--disable-blink-features=AutomationControlled',
      '--no-sandbox',
      '--disable-setuid-sandbox',
    ],
  });

  const context = await browser.newContext({
    storageState: STATE_FILE,
    viewport: { width: 1920, height: 1080 },
    userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36',
  });

  const page = await context.newPage();
  page.setDefaultTimeout(180000);

  // Intercept GraphQL requests to capture the Authorization token and learn query formats
  let authToken = '';
  let capturedDeviceUuid = '';
  const capturedQueries: Map<string, { query: string; variables: any }> = new Map();

  page.on('request', (request: any) => {
    if (request.url().includes('graphql')) {
      const headers = request.headers();
      if (headers['authorization'] && !authToken) {
        authToken = headers['authorization'];
        capturedDeviceUuid = headers['device-uuid'] || '';
        logInfo('auth_captured', 'Captured auth token from app request', {
          tokenPrefix: authToken.substring(0, 20) + '...',
          deviceUuid: capturedDeviceUuid,
        });
      }

      // Capture query formats
      try {
        const postData = request.postData();
        if (postData) {
          const body = JSON.parse(postData);
          if (body.operationName && body.query) {
            capturedQueries.set(body.operationName, {
              query: body.query,
              variables: body.variables || {},
            });
          }
        }
      } catch {
        // Ignore parse errors
      }
    }
  });

  // Navigate to app to establish session
  logInfo('navigation', 'Establishing session...');
  await page.goto('https://app.monarch.com/dashboard', {
    waitUntil: 'domcontentloaded',
    timeout: 180000
  });

  // Wait for the app to load and make its first GraphQL request (captures auth token)
  await page.waitForTimeout(5000);

  // Navigate to transactions page to capture query formats
  logInfo('navigation', 'Navigating to transactions page to capture query format...');
  await page.goto('https://app.monarch.com/transactions', {
    waitUntil: 'domcontentloaded',
    timeout: 60000
  });
  await page.waitForTimeout(3000);

  // Navigate to investments page to capture portfolio/holdings query
  logInfo('navigation', 'Navigating to investments page to capture query format...');
  await page.goto('https://app.monarch.com/investments', {
    waitUntil: 'domcontentloaded',
    timeout: 60000
  });
  await page.waitForTimeout(3000);

  // Log captured queries
  logDebug('captured_queries', 'Captured query formats', {
    operations: Array.from(capturedQueries.keys()),
  });

  // Use the captured transactions query if available
  const transactionsQueryData = capturedQueries.get('Web_GetTransactionsList');
  if (transactionsQueryData) {
    logInfo('query_captured', 'Captured transactions query format', {
      query: transactionsQueryData.query.substring(0, 500),
      variableKeys: Object.keys(transactionsQueryData.variables),
    });
  }

  // Log holdings-related queries
  for (const [name, data] of capturedQueries.entries()) {
    if (name.toLowerCase().includes('holding') || name.toLowerCase().includes('portfolio') || name.toLowerCase().includes('investment')) {
      logInfo('holdings_query', `Captured holdings-related query: ${name}`, {
        query: data.query.substring(0, 800),
        variableKeys: Object.keys(data.variables),
      });
    }
    if (name.toLowerCase().includes('snapshot') || name.toLowerCase().includes('aggregate')) {
      logInfo('snapshot_query', `Captured snapshot query: ${name}`, {
        query: data.query.substring(0, 800),
        variableKeys: Object.keys(data.variables),
        variables: data.variables,
      });
    }
  }

  // Verify we captured the auth token
  if (!authToken) {
    logError('no_auth_token', 'Failed to capture auth token from app requests');
    await browser.close();
    process.exit(1);
  }

  // Check if we got redirected to login
  const currentUrl = page.url();
  if (currentUrl.includes('/login')) {
    logError('session_expired', 'Session expired - redirected to login page');
    await browser.close();
    process.exit(1);
  }

  logInfo('session_valid', 'Session valid, starting data fetch', {
    deviceUuid: capturedDeviceUuid,
    url: currentUrl,
  });

  const results: any = {
    timestamp: new Date().toISOString(),
    accounts: [],
    transactions: [],
    holdings: [],
    netWorthHistory: [],
  };

  try {
    // Fetch accounts
    logInfo('fetch_accounts', 'Fetching accounts...');
    const accountsTimer = timer('accounts');
    const accountsData = await executeGraphQL<any>(
      page, 'Web_GetAccountsPage', GET_ACCOUNTS_QUERY, {},
      authToken, capturedDeviceUuid
    );
    results.accounts = accountsData.accounts;
    logInfo('fetch_accounts_done', `Fetched ${results.accounts.length} accounts`, { duration_ms: accountsTimer() });

    // Fetch transactions (with optional high watermark)
    const sinceDate = process.env.TRANSACTIONS_SINCE || undefined;
    logInfo('fetch_transactions', 'Fetching transactions...', { since: sinceDate || 'all' });
    const txnTimer = timer('transactions');
    results.transactions = await fetchAllTransactions(page, authToken, capturedDeviceUuid, sinceDate);
    logInfo('fetch_transactions_done', `Fetched ${results.transactions.length} transactions`, { duration_ms: txnTimer() });

    // Fetch holdings
    logInfo('fetch_holdings', 'Fetching holdings...');
    const holdingsTimer = timer('holdings');
    const holdingsData = await executeGraphQL<any>(
      page, 'Web_GetPortfolio', GET_HOLDINGS_QUERY, { portfolioInput: {} },
      authToken, capturedDeviceUuid
    );
    // Transform from aggregateHoldings.edges.node structure
    results.holdings = holdingsData.portfolio?.aggregateHoldings?.edges?.map((e: any) => e.node) || [];
    results.portfolioPerformance = holdingsData.portfolio?.performance;
    logInfo('fetch_holdings_done', `Fetched ${results.holdings.length} holdings`, { duration_ms: holdingsTimer() });

    // Fetch net worth history
    const historyDays = parseInt(process.env.HISTORY_DAYS || '365', 10);
    const startDate = new Date(Date.now() - historyDays * 24 * 60 * 60 * 1000).toISOString().split('T')[0];
    logInfo('fetch_history', 'Fetching net worth history...', { startDate, historyDays });
    const historyTimer = timer('history');
    const historyData = await executeGraphQL<any>(
      page, 'Common_GetAggregateSnapshots', GET_AGGREGATE_SNAPSHOTS_QUERY,
      { filters: { startDate, endDate: null, accountType: null, useAdaptiveGranularity: true } },
      authToken, capturedDeviceUuid
    );
    results.netWorthHistory = historyData.aggregateSnapshots || [];
    logInfo('fetch_history_done', `Fetched ${results.netWorthHistory.length} history snapshots`, { duration_ms: historyTimer() });

  } catch (error) {
    logError('scrape_error', `Error during scrape: ${error}`);
    await browser.close();
    process.exit(1);
  }

  // Save results
  const outputPath = join(OUTPUT_DIR, 'monarch-data.json');
  writeFileSync(outputPath, JSON.stringify(results, null, 2));
  logInfo('output_saved', `Data saved to ${outputPath}`, { path: outputPath });

  // Calculate summary
  const netWorth = results.accounts
    .filter((a: any) => a.includeInNetWorth && !a.isHidden)
    .reduce((sum: number, a: any) => {
      const balance = parseFloat(a.displayBalance || a.currentBalance || 0);
      return sum + (a.isAsset ? balance : -Math.abs(balance));
    }, 0);

  logInfo('scrape_complete', 'Scrape complete', {
    duration_ms: totalTimer(),
    accounts: results.accounts.length,
    transactions: results.transactions.length,
    holdings: results.holdings.length,
    netWorth: netWorth.toFixed(2),
  });

  await browser.close();

  // Print summary
  console.log(JSON.stringify({
    success: true,
    accounts_count: results.accounts.length,
    transactions_count: results.transactions.length,
    holdings_count: results.holdings.length,
    net_worth: netWorth,
    output_file: outputPath,
  }));
}

main().catch((error) => {
  logError('fatal_error', `Fatal error: ${error}`);
  process.exit(1);
});
