//! GraphQL query definitions for Monarch Money API
//! Field sets were checked against the live API in 2026-10. Monarch answers a
//! query containing an unknown field with an opaque 400, so add fields one at
//! a time when extending these.

/// Fetch all accounts with balances, institution and sync metadata
pub const GET_ACCOUNTS_QUERY: &str = r#"
query Web_GetAccountsPage {
  accounts {
    id
    displayName
    mask
    order
    icon
    logoUrl
    notes
    isHidden
    isAsset
    isManual
    isTaxable
    includeInNetWorth
    includeBalanceInNetWorth
    hideFromList
    hideTransactionsFromReports
    syncDisabled
    deactivatedAt
    createdAt
    updatedAt
    displayLastUpdatedAt
    currentBalance
    displayBalance
    signedBalance
    limit
    apr
    interestRate
    minimumPayment
    plannedPayment
    dataProvider
    dataProviderAccountId
    holdingsCount
    transactionsCount
    manualInvestmentsTrackingMethod
    type {
      name
      display
      group
      __typename
    }
    subtype {
      name
      display
      __typename
    }
    credential {
      id
      updateRequired
      dataProvider
      disconnectedFromDataProviderAt
      syncDisabledAt
      syncDisabledReason
      institution {
        id
        name
        status
        url
        __typename
      }
      __typename
    }
    institution {
      id
      name
      url
      primaryColor
      __typename
    }
    __typename
  }
}
"#;

/// Fetch Monarch's full daily balance history for one account
pub const GET_ACCOUNT_SNAPSHOTS_QUERY: &str = r#"
query AccountDetails_getSnapshots($id: UUID!) {
  snapshots: snapshotsForAccount(accountId: $id) {
    date
    signedBalance
    __typename
  }
}
"#;

/// Fetch paginated transactions with full details
/// Note: recurrenceGroup is deliberately omitted; it makes each page take ~20x longer
pub const GET_TRANSACTIONS_QUERY: &str = r#"
query Web_GetTransactionsList($offset: Int, $limit: Int, $filters: TransactionFilterInput, $orderBy: TransactionOrdering) {
  allTransactions(filters: $filters) {
    totalCount
    results(offset: $offset, limit: $limit, orderBy: $orderBy) {
      id
      amount
      pending
      date
      originalDate
      hideFromReports
      hiddenByAccount
      plaidName
      dataProviderDescription
      notes
      isRecurring
      isManual
      reviewStatus
      needsReview
      reviewedAt
      isSplitTransaction
      hasSplitTransactions
      createdAt
      updatedAt
      originalTransaction {
        id
        __typename
      }
      splitTransactions {
        id
        __typename
      }
      attachments {
        id
        __typename
      }
      goal {
        id
        name
        __typename
      }
      account {
        id
        displayName
        __typename
      }
      category {
        id
        name
        icon
        group {
          id
          type
          __typename
        }
        __typename
      }
      merchant {
        id
        name
        logoUrl
        __typename
      }
      tags {
        id
        name
        color
        order
        __typename
      }
      ownedByUser {
        id
        displayName
        __typename
      }
      __typename
    }
    __typename
  }
}
"#;

/// Fetch all category groups and categories
pub const GET_CATEGORIES_QUERY: &str = r#"
query Common_GetCategories($includeSystemDisabledCategories: Boolean) {
  categoryGroups {
    id
    name
    order
    type
    groupLevelBudgetingEnabled
    budgetVariability
    rolloverPeriod {
      id
      startMonth
      startingBalance
      frequency
      targetAmount
      type
      __typename
    }
    categories {
      id
      __typename
    }
    __typename
  }
  categories(includeDisabledSystemCategories: $includeSystemDisabledCategories) {
    id
    name
    order
    icon
    isSystemCategory
    excludeFromBudget
    budgetVariability
    isDisabled
    group {
      id
      type
      __typename
    }
    rolloverPeriod {
      id
      startMonth
      startingBalance
      frequency
      targetAmount
      type
      __typename
    }
    __typename
  }
}
"#;

/// Fetch all household transaction tags
pub const GET_HOUSEHOLD_TAGS_QUERY: &str = r#"
query Common_GetHouseholdTransactionTags {
  householdTransactionTags {
    id
    name
    color
    __typename
  }
}
"#;

/// Fetch the investment portfolio: performance, each security held, and the
/// per-account positions and tax lots behind it
pub const GET_HOLDINGS_QUERY: &str = r#"
query Web_GetPortfolio($portfolioInput: PortfolioInput) {
  portfolio(input: $portfolioInput) {
    performance {
      totalValue
      totalChangePercent
      totalChangeDollars
      oneDayChangePercent
      historicalChart {
        date
        returnPercent
        __typename
      }
      benchmarks {
        security {
          id
          ticker
          name
          oneDayChangePercent
          __typename
        }
        historicalChart {
          date
          returnPercent
          __typename
        }
        __typename
      }
      __typename
    }
    aggregateHoldings {
      edges {
        node {
          id
          quantity
          costBasis
          totalValue
          securityPriceChangeDollars
          securityPriceChangePercent
          lastSyncedAt
          holdings {
            id
            type
            typeDisplay
            name
            ticker
            closingPrice
            closingPriceUpdatedAt
            quantity
            value
            costBasis
            userCostBasis
            account {
              id
              mask
              icon
              logoUrl
              institution {
                id
                name
                __typename
              }
              type {
                name
                display
                __typename
              }
              subtype {
                name
                display
                __typename
              }
              displayName
              order
              currentBalance
              __typename
            }
            taxLots {
              id
              createdAt
              acquisitionDate
              acquisitionQuantity
              costBasisPerUnit
              __typename
            }
            allocationCategoryWeights {
              weight
              category {
                id
                __typename
              }
              __typename
            }
            securityClassification {
              id
              categorySlug
              slices {
                id
                allocationSlug
                percentage
                breakdownMode
                referenceSecurityId
                subSlices {
                  id
                  allocationSlug
                  percentage
                  breakdownMode
                  referenceSecurityId
                  __typename
                }
                __typename
              }
              __typename
            }
            holdingClassification {
              id
              categorySlug
              slices {
                id
                allocationSlug
                percentage
                breakdownMode
                referenceSecurityId
                subSlices {
                  id
                  allocationSlug
                  percentage
                  breakdownMode
                  referenceSecurityId
                  __typename
                }
                __typename
              }
              __typename
            }
            __typename
          }
          security {
            id
            name
            ticker
            currentPrice
            currentPriceUpdatedAt
            closingPrice
            type
            typeDisplay
            assetClass
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
}
"#;

/// Fetch recurring transaction streams (subscriptions, bills, income) together
/// with their upcoming occurrences, which carry the next due date and account
pub const GET_RECURRING_STREAMS_QUERY: &str = r#"
query Common_GetRecurringStreams($startDate: Date!, $endDate: Date!) {
  recurringTransactionStreams(includePending: true, includeLiabilities: true) {
    stream {
      id
      frequency
      amount
      name
      logoUrl
      recurringType
      reviewStatus
      isActive
      merchant {
        id
        name
        logoUrl
        __typename
      }
      __typename
    }
    __typename
  }
  recurringTransactionItems(
    startDate: $startDate
    endDate: $endDate
    includeLiabilities: true
    filters: {isCompleted: false}
  ) {
    stream {
      id
      __typename
    }
    date
    account {
      id
      __typename
    }
    __typename
  }
}
"#;

/// Fetch historical aggregate snapshots for net worth charting
/// Note: Uses filters object instead of separate date parameters
pub const GET_AGGREGATE_SNAPSHOTS_QUERY: &str = r#"
query Common_GetAggregateSnapshots($filters: AggregateSnapshotFilters) {
  aggregateSnapshots(filters: $filters) {
    date
    balance
    assetsBalance
    liabilitiesBalance
    __typename
  }
}
"#;

/// Ask Monarch to re-sync every connected account with its institution
pub const FORCE_REFRESH_ALL_ACCOUNTS_MUTATION: &str = r#"
mutation Common_ForceRefreshAccountsMutation($input: ForceRefreshAllAccountsInput) {
  forceRefreshAllAccounts(input: $input) {
    success
    forceRefreshOperationId
    errors {
      message
      code
      __typename
    }
    __typename
  }
}
"#;

/// Progress of a force refresh started by FORCE_REFRESH_ALL_ACCOUNTS_MUTATION
pub const GET_FORCE_REFRESH_OPERATION_QUERY: &str = r#"
query Common_ForceRefreshOperationQuery($id: ID!) {
  forceRefreshOperation(id: $id) {
    id
    state
    completedAccountCount
    totalAccountCount
    accounts {
      accountId
      state
      newTransactionCount
      updatedTransactionCount
      timedOut
      errorMessage
      __typename
    }
    __typename
  }
}
"#;

/// Whether any account is still syncing with its institution
pub const HAS_ACCOUNTS_SYNCING_QUERY: &str = r#"
query Common_ForceRefreshAccountsQuery {
  hasAccountsSyncing
}
"#;

/// Budget plan and actuals by category and group for a range of months
pub const GET_BUDGET_QUERY: &str = r#"
query Common_BudgetDataQuery($startDate: Date!, $endDate: Date!) {
  budgetSystem
  budgetStatus {
    ...BudgetStatusFields
    __typename
  }
  budgetData(startMonth: $startDate, endMonth: $startDate) {
    ...BudgetDataFields
    __typename
  }
  categoryGroups {
    ...BudgetCategoryGroupFields
    __typename
  }
  goalsV2 {
    ...BudgetDataGoalsV2Fields
    __typename
  }
  savingsGoalMonthlyBudgetAmounts(startMonth: $startDate, endMonth: $endDate) {
    ...SavingsGoalMonthlyBudgetAmountsFields
    __typename
  }
  debtPaydownMonthlyBudgetAmounts(startMonth: $startDate, endMonth: $endDate) {
    ...DebtPaydownMonthlyBudgetAmountsFields
    __typename
  }
}

fragment BudgetDataMonthlyAmountsFields on BudgetMonthlyAmounts {
  month
  plannedCashFlowAmount
  plannedSetAsideAmount
  actualAmount
  remainingAmount
  previousMonthRolloverAmount
  rolloverType
  cumulativeActualAmount
  rolloverTargetAmount
  __typename
}

fragment BudgetMonthlyAmountsByCategoryFields on BudgetCategoryMonthlyAmounts {
  category {
    id
    __typename
  }
  monthlyAmounts {
    ...BudgetDataMonthlyAmountsFields
    __typename
  }
  __typename
}

fragment BudgetMonthlyAmountsByCategoryGroupFields on BudgetCategoryGroupMonthlyAmounts {
  categoryGroup {
    id
    __typename
  }
  monthlyAmounts {
    ...BudgetDataMonthlyAmountsFields
    __typename
  }
  __typename
}

fragment BudgetMonthlyAmountsForFlexExpenseFields on BudgetFlexMonthlyAmounts {
  budgetVariability
  monthlyAmounts {
    ...BudgetDataMonthlyAmountsFields
    __typename
  }
  __typename
}

fragment BudgetDataTotalsByMonthFields on BudgetTotals {
  actualAmount
  plannedAmount
  previousMonthRolloverAmount
  remainingAmount
  __typename
}

fragment BudgetTotalsByMonthFields on BudgetMonthTotals {
  month
  totalIncome {
    ...BudgetDataTotalsByMonthFields
    __typename
  }
  totalExpenses {
    ...BudgetDataTotalsByMonthFields
    __typename
  }
  totalFixedExpenses {
    ...BudgetDataTotalsByMonthFields
    __typename
  }
  totalNonMonthlyExpenses {
    ...BudgetDataTotalsByMonthFields
    __typename
  }
  totalFlexibleExpenses {
    ...BudgetDataTotalsByMonthFields
    __typename
  }
  __typename
}

fragment BudgetRolloverPeriodFields on BudgetRolloverPeriod {
  id
  startMonth
  endMonth
  startingBalance
  targetAmount
  frequency
  type
  __typename
}

fragment BudgetCategoryFields on Category {
  id
  name
  icon
  order
  budgetVariability
  excludeFromBudget
  isSystemCategory
  updatedAt
  group {
    id
    type
    budgetVariability
    groupLevelBudgetingEnabled
    __typename
  }
  rolloverPeriod {
    ...BudgetRolloverPeriodFields
    __typename
  }
  __typename
}

fragment DebtAccountFields on Account {
  id
  displayName
  displayBalance
  icon
  logoUrl
  includeInNetWorth
  isHidden
  displayLastUpdatedAt
  apr
  interestRate
  minimumPayment
  plannedPayment
  excludeFromDebtPaydown
  type {
    name
    display
    __typename
  }
  subtype {
    name
    display
    __typename
  }
  __typename
}

fragment BudgetStatusFields on BudgetStatus {
  hasBudget
  hasTransactions
  willCreateBudgetFromEmptyDefaultCategories
  __typename
}

fragment BudgetDataFields on BudgetData {
  monthlyAmountsByCategory {
    ...BudgetMonthlyAmountsByCategoryFields
    __typename
  }
  monthlyAmountsByCategoryGroup {
    ...BudgetMonthlyAmountsByCategoryGroupFields
    __typename
  }
  monthlyAmountsForFlexExpense {
    ...BudgetMonthlyAmountsForFlexExpenseFields
    __typename
  }
  totalsByMonth {
    ...BudgetTotalsByMonthFields
    __typename
  }
  __typename
}

fragment BudgetCategoryGroupFields on CategoryGroup {
  id
  name
  order
  type
  budgetVariability
  updatedAt
  groupLevelBudgetingEnabled
  categories {
    ...BudgetCategoryFields
    __typename
  }
  rolloverPeriod {
    id
    type
    startMonth
    endMonth
    startingBalance
    frequency
    targetAmount
    __typename
  }
  __typename
}

fragment BudgetDataGoalsV2Fields on GoalV2 {
  id
  name
  archivedAt
  completedAt
  priority
  imageStorageProvider
  imageStorageProviderId
  plannedContributions(startMonth: $startDate, endMonth: $endDate) {
    id
    month
    amount
    __typename
  }
  monthlyContributionSummaries(startMonth: $startDate, endMonth: $endDate) {
    month
    sum
    __typename
  }
  __typename
}

fragment SavingsGoalMonthlyBudgetAmountsFields on SavingsGoalMonthlyBudgetAmounts {
  id
  savingsGoal {
    id
    name
    type
    status
    archivedAt
    priority
    targetDate
    imageStorageProvider
    imageStorageProviderId
    __typename
  }
  monthlyAmounts {
    id
    month
    plannedAmount
    actualAmount
    remainingAmount
    totalPlannedAmount
    totalActualAmount
    totalRemainingAmount
    __typename
  }
  __typename
}

fragment DebtPaydownMonthlyBudgetAmountsFields on DebtPaydownMonthlyBudgetAmounts {
  id
  account {
    ...DebtAccountFields
    __typename
  }
  monthlyAmounts {
    id
    month
    plannedAmount
    actualAmount
    remainingAmount
    __typename
  }
  __typename
}
"#;

/// Savings goals with balances and per-account allocations
pub const GET_GOALS_QUERY: &str = r#"
query Common_SavingsGoalsWithThisMonthBalances {
  savingsGoals {
    ...GoalSummaryFields
    __typename
  }
  goalsBalanceThisMonth
  currentTotalBalanceForGoals
}

fragment NewAccountLogoFields on Account {
  id
  dataProvider
  logoUrl
  type {
    name
    display
    __typename
  }
  subtype {
    name
    __typename
  }
  institution {
    id
    logo
    primaryColor
    __typename
  }
  __typename
}

fragment GoalSummaryFields on SavingsGoal {
  id
  type
  name
  createdAt
  archivedAt
  imageStorageProvider
  imageStorageProviderId
  status
  progress
  currentBalance
  targetDate
  targetAmount
  hasFutureBudgetDifferentFromCurrentMonth
  currentMonthActualBudgetAmount
  currentMonthPlannedContributionAmount
  plannedMonthlyContribution
  spendingTotal
  netContribution
  netContributionWithSpending
  netContributionWithoutSpending
  balanceThisMonth
  estimatedMonthsUntilCompletion
  forecastedCompletionDate
  isSinkingFund
  priority
  allocationAmountsByAccount {
    goalId
    adjustmentAmount
    totalAmount
    spendingAmount
    contributionsAmount
    withdrawalsAmount
    account {
      icon
      displayName
      displayBalance
      linkedGoal {
        id
        __typename
      }
      subtype {
        name
        display
        __typename
      }
      ...NewAccountLogoFields
      __typename
    }
    __typename
  }
  __typename
}
"#;

/// Credit score history
pub const GET_CREDIT_SCORE_QUERY: &str = r#"
query Common_GetSpinwheelCreditScoreSnapshots {
  me {
    id
    __typename
  }
  myHousehold {
    id
    users {
      id
      name
      displayName
      profilePictureUrl
      __typename
    }
    __typename
  }
  spinwheelUser {
    id
    user {
      id
      name
      displayName
      __typename
    }
    onboardingStatus
    onboardingErrorMessage
    ...Common_SpinwheelUserFields
    __typename
  }
  creditScoreSnapshots {
    reportedDate
    score
    user {
      id
      __typename
    }
    __typename
  }
}

fragment Common_SpinwheelUserFields on SpinwheelUser {
  id
  spinwheelUserId
  creditScoreRefreshSubscriptionId
  creditScoreTrackingStatus
  isBillSyncTrackingEnabled
  __typename
}
"#;

/// Household profile
pub const GET_HOUSEHOLD_QUERY: &str = r#"
query Common_GetMyHousehold {
  myHousehold {
    ...HouseholdFields
    __typename
  }
}

fragment HouseholdFields on Household {
  address
  city
  id
  name
  state
  zipCode
  country
  __typename
}
"#;

/// Household members
pub const GET_HOUSEHOLD_MEMBERS_QUERY: &str = r#"
query Common_GetHouseholdMembers {
  me {
    id
    __typename
  }
  myHousehold {
    id
    users {
      id
      name
      displayName
      householdRole
      profilePictureUrl
      avatarColor
      __typename
    }
    __typename
  }
}
"#;

/// Delete a merchant, moving its transactions and rules to another merchant.
/// With $moveToId this is how the web app merges two merchants.
pub const DELETE_MERCHANT_MUTATION: &str = r#"
mutation Common_DeleteMerchant($merchantId: ID!, $moveToId: ID) {
  deleteMerchant(id: $merchantId, moveRelationsToMerchantId: $moveToId) {
    success
    __typename
  }
}
"#;

/// Rename a merchant
pub const UPDATE_MERCHANT_MUTATION: &str = r#"
mutation Common_UpdateMerchant($input: UpdateMerchantInput!) {
  updateMerchant(input: $input) {
    merchant {
      id
      name
      transactionCount
      __typename
    }
    errors {
      message
      code
      __typename
    }
    __typename
  }
}
"#;

/// Transaction rules: what they match and what they change
pub const GET_TRANSACTION_RULES_QUERY: &str = r#"
query Web_GetTransactionRules {
  transactionRules {
    id
    order
    merchantCriteriaUseOriginalStatement
    merchantCriteria {
      operator
      value
      __typename
    }
    originalStatementCriteria {
      operator
      value
      __typename
    }
    merchantNameCriteria {
      operator
      value
      __typename
    }
    amountCriteria {
      operator
      isExpense
      value
      valueRange {
        lower
        upper
        __typename
      }
      __typename
    }
    categoryIds
    accountIds
    setMerchantAction {
      id
      name
      __typename
    }
    setCategoryAction {
      id
      name
      __typename
    }
    addTagsAction {
      id
      name
      __typename
    }
    setHideFromReportsAction
    reviewStatusAction
    lastAppliedAt
    recentApplicationCount
    __typename
  }
}
"#;

/// Create a transaction rule
pub const CREATE_TRANSACTION_RULE_MUTATION: &str = r#"
mutation Common_CreateTransactionRuleMutationV2($input: CreateTransactionRuleInput!) {
  createTransactionRuleV2(input: $input) {
    errors {
      fieldErrors {
        field
        messages
        __typename
      }
      message
      code
      __typename
    }
    __typename
  }
}
"#;

/// Search the household's merchants by name
pub const SEARCH_MERCHANTS_QUERY: &str = r#"
query Web_GetMerchantSettingsPage($offset: Int, $orderBy: MerchantOrdering, $search: String, $filters: MerchantFilters) {
  merchants(offset: $offset, orderBy: $orderBy, search: $search, filters: $filters) {
    id
    name
    transactionCount
    __typename
  }
}
"#;

/// Change one transaction's merchant and/or category
pub const UPDATE_TRANSACTION_MUTATION: &str = r#"
mutation Common_UpdateTransaction($input: UpdateTransactionMutationInput!) {
  updateTransaction(input: $input) {
    transaction {
      id
      category {
        id
        name
        __typename
      }
      merchant {
        id
        name
        __typename
      }
      __typename
    }
    errors {
      message
      code
      __typename
    }
    __typename
  }
}
"#;

/// Delete a transaction rule
pub const DELETE_TRANSACTION_RULE_MUTATION: &str = r#"
mutation Common_DeleteTransactionRule($id: ID!) {
  deleteTransactionRule(id: $id) {
    deleted
    errors {
      message
      code
      __typename
    }
    __typename
  }
}
"#;

/// Delete one transaction
pub const DELETE_TRANSACTION_MUTATION: &str = r#"
mutation Common_DeleteTransactionMutation($input: DeleteTransactionMutationInput!) {
  deleteTransaction(input: $input) {
    deleted
    errors {
      message
      code
      __typename
    }
    __typename
  }
}
"#;

/// Create a transaction tag
pub const CREATE_TAG_MUTATION: &str = r#"
mutation Common_CreateTransactionTag($input: CreateTransactionTagInput!) {
  createTransactionTag(input: $input) {
    tag {
      id
      name
      color
      __typename
    }
    errors {
      message
      __typename
    }
    __typename
  }
}
"#;

/// The tags and merchant on one transaction
pub const GET_TRANSACTION_QUERY: &str = r#"
query GetTransactionDrawer($id: UUID!) {
  getTransaction(id: $id) {
    id
    amount
    merchant {
      id
      name
      __typename
    }
    tags {
      id
      name
      __typename
    }
    __typename
  }
}
"#;

/// Replace the set of tags on a transaction
pub const SET_TRANSACTION_TAGS_MUTATION: &str = r#"
mutation Web_SetTransactionTags($input: SetTransactionTagsInput!) {
  setTransactionTags(input: $input) {
    errors {
      message
      code
      __typename
    }
    transaction {
      id
      tags {
        id
        __typename
      }
      __typename
    }
    __typename
  }
}
"#;

/// Split a transaction into parts that can be categorized separately
pub const SPLIT_TRANSACTION_MUTATION: &str = r#"
mutation Common_SplitTransactionMutation($input: UpdateTransactionSplitMutationInput!) {
  updateTransactionSplit(input: $input) {
    errors {
      message
      code
      __typename
    }
    transaction {
      id
      hasSplitTransactions
      splitTransactions {
        id
        amount
        category {
          id
          name
          __typename
        }
        __typename
      }
      __typename
    }
    __typename
  }
}
"#;
