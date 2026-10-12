# nestegg, the terminal app

A terminal interface over the Postgres mirror that the sync in this repo maintains.
It reads the mirrored database. Optional merchant/category edits go through the
authenticated [write API](../docs/WRITE_API.md) on the machine that maintains
the Monarch session; the terminal app never receives that session.

```bash
cargo install --git https://github.com/brandon-tallent/nestegg nestegg --locked
# or, from a checkout: cargo install --path tui --locked

nestegg --database postgresql://nestegg:PASSWORD@localhost:5432/nestegg
export DATABASE_URL=postgresql://...   # or set it once
nestegg --anonymize                    # start with names and amounts swapped for stand-ins
nestegg --demo                         # a made-up household; needs no database
```

Without `--database` or `DATABASE_URL`, the URL is looked up with `kubectl`
for a stack deployed from `k8s/`: it is read from the `nestegg-secrets` secret
and pointed at the `postgres` service's cluster IP, so it works from a
cluster node. `--namespace` picks another namespace.

## Screens

| Key | Screen | What it shows |
|-----|--------|---------------|
| 1 | Dashboard | Net worth, spending this month against last, recent transactions, upcoming recurring, investments, credit score, goals |
| 2 | Accounts | Every account grouped by type, with balance history, payments expected through month end, and recent activity for the selection |
| 3 | Transactions | The full history; `/` searches, `f` filters, Enter shows details |
| 4 | Cash Flow | Income and spending by month, broken down by category, group or merchant |
| 5 | Reports | Spending by category across recent months |
| 6 | Budget | This month's plan against actuals |
| 7 | Recurring | Bills, subscriptions and income with their next dates |
| 8 | Goals | Savings goals |
| 9 | Investments | Portfolio value, allocation, holdings, and positions by account (`v`) |
| 0 | Equity | Private company holdings from Carta: option grants, shares held, vesting ahead, exercises and sales, and value at other share prices; `v` moves between the lists, Enter opens the selection |

Mouse: click the sidebar or tabs to switch screens, click rows to select,
double-click to open, and use the wheel to scroll. Footer controls are clickable.
Click outside a detail dialog, or its close control, to dismiss it.

On Transactions, `t` marks/unmarks a row (`T` marks every visible row, `u`
unmarks all). Clicking the row's leftmost marker does the same. Marks stay
attached to transaction IDs when you search, filter, or reload. `m` changes
the marked transactions' merchant; `C` changes their category. Enter reviews
the change, then Enter applies it to Monarch; Esc cancels. Category choices
come from Monarch's mirrored category list. Successful updates refresh the
database and clear their marks; failed updates remain marked. Editing is
disabled in anonymized mode.

Esc closes transaction details first, then returns to Accounts, Cash Flow, or
Recurring when that screen opened a drill-down.

`?` lists every key. `[` and `]` change the chart range; `r` re-reads the database.

`/` searches the screen you are on, narrowing its list as you type: accounts by
name, institution, type or last four digits; recurring payments by merchant,
account or frequency; holdings by symbol or name; categories on Cash Flow,
Reports and Budget; goals by name. On the Dashboard it searches all
transactions. Words match in any order, ignoring case and punctuation
(`trader joes`, `inshape`), and `>100`, `<50`, `$45` or `$45.99` test the
amount. Transactions also match dates (`2026-08`, `aug`). Enter keeps the
search, Esc clears it.

Once a Carta capture is loaded, the equity account Monarch tracks by hand
takes its figures from Carta instead: the balance is what is vested (shares
plus exercisable options at their spread) at the latest share price, and the
history is rebuilt from the vesting, exercises, tender sales and share prices
on record. A share price is known only for days something was recorded, and
holds until the next one.

On the Equity screen, Enter opens whatever is selected. A grant shows its
type, expiry, vesting terms, and its options at each stage divided between ISO
and NSO, with what each part costs to exercise. A share certificate shows
where its shares came from (the grant, ISO or NSO, and what they were worth
over their cost when exercised) and the dates from which a sale is long-term
and, for ISO shares, a qualifying disposition. When Carta cancels a
certificate to issue smaller ones, as when part of it is sold, the pieces
carry no cost or origin of their own; they are traced back to the certificate
they were cut from. A tender sale lists the certificates sold into it with
each one's cost, proceeds and gain. The tax labels state the holding-period
rules only; they are not a tax calculation.

`o` includes or excludes transactions tagged `One-time` in Monarch (a tax
settlement, a share sale, a capital project) from income and spending on the
Dashboard, Cash Flow and Reports screens. They are excluded by default, and the
Cash Flow summary says what was set aside. Tag or untag in Monarch, or with
`monarch-scraper --tag-transaction ID=One-time`.

`a` toggles anonymized mode for showing your own screen to someone: merchants,
accounts and funds get invented names, every amount is rescaled, a share of
the one-off transactions is left out and the rest move a few days later, each
investment account's balance is dealt afresh among its holdings, and the Equity
screen shows invented grants worth roughly what yours are. It stays consistent
enough that charts, totals and recurring payments still make sense. The
disguise is fixed for the session and different on the next run.

It is a disguise of your data, not a replacement for it: the categories, the
accounts and how they compare, and the shape of every chart are still yours.
For a screenshot anyone may see, use `--demo` instead. It shows a household
that does not exist (two salaries, a mortgage, two credit cards, retirement
and brokerage accounts, options in a private company) and reads nothing from
a database. Its history runs up to today, and it is the same on every run.

## Checking a layout without a terminal

```bash
nestegg --demo --dump --screen investments --keys "v,j,j" --size 140x40
```

renders one frame as plain text after pressing the given keys.

## Upcoming payments

The Accounts screen lists payments expected for the rest of the month. These
come from `src/forecast.rs`, which infers recurring payments from transaction
history rather than using Monarch's recurring feature: a merchant an account
pays on a steady weekly, biweekly, monthly, quarterly or yearly rhythm is a
stream, and its next dates follow from the last payment. A `~` marks an
estimated amount for bills that vary.

```bash
nestegg --streams   # every detected stream, for judging the detection
```
