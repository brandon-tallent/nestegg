//! Terminal interface over the Monarch Money mirror database

mod anonymize;
mod app;
mod data;
mod demo;
mod dice;
mod equity;
mod fmt;
mod payoff;
mod forecast;
mod search;
mod theme;
mod mouse;
mod edits;
mod ui;

use anyhow::{bail, Context, Result};
use app::{App, Screen};
use clap::Parser;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, EnableMouseCapture, DisableMouseCapture};
use ratatui::crossterm::execute;
use ratatui::Terminal;
use std::process::Command;
use std::time::Duration;

/// How long to wait for a key before redrawing (keeps "synced Nm ago" current)
const TICK: Duration = Duration::from_secs(30);

/// The Postgres service in the cluster, for `kubectl` discovery. The secret
/// and the service's DNS name follow from the namespace.
const KUBE_SERVICE: &str = "postgres";

#[derive(Parser, Debug)]
#[command(name = "nestegg", about = "Browse your mirrored personal finance data in the terminal", version)]
struct Args {
    /// PostgreSQL URL of the mirror. When omitted, it is looked up from the
    /// cluster with kubectl.
    #[arg(short, long, env = "DATABASE_URL")]
    database: Option<String>,

    /// Kubernetes namespace the mirror is deployed in, for that lookup
    #[arg(long, env = "NESTEGG_NAMESPACE", default_value = "nestegg")]
    namespace: String,

    /// Show a made-up household instead of reading a database: for
    /// screenshots, and for trying the interface before there is a mirror
    #[arg(long)]
    demo: bool,

    /// Start with names and amounts swapped for stand-ins, for showing your
    /// own screen to others (the `a` key toggles this while running)
    #[arg(long)]
    anonymize: bool,

    /// Screen to open on (dashboard, accounts, transactions, cashflow,
    /// reports, budget, recurring, goals, investments, equity)
    #[arg(long, default_value = "dashboard")]
    screen: String,

    /// Print the recurring payments detected from transaction history and exit
    #[arg(long)]
    streams: bool,

    /// Render one frame of --screen as plain text and exit
    #[arg(long)]
    dump: bool,

    /// Keys to press before the --dump frame is rendered, comma separated
    /// (single characters, or enter, esc, tab, space, backspace, up, down, left, right)
    #[arg(long, default_value = "")]
    keys: String,

    /// Size of the --dump frame, as COLUMNSxROWS
    #[arg(long, default_value = "140x45")]
    size: String,
}

fn kubectl(args: &[&str]) -> Result<String> {
    let output = Command::new("kubectl")
        .args(args)
        .output()
        .context("Could not run kubectl")?;
    if !output.status.success() {
        bail!("kubectl {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

/// Read the database URL from the cluster secret, pointed at the service's
/// cluster IP since the in-cluster DNS name does not resolve from the host
fn discover_database_url(namespace: &str) -> Result<String> {
    let secret = format!("{namespace}-secrets");
    let url = kubectl(&[
        "-n",
        namespace,
        "get",
        "secret",
        &secret,
        "-o",
        "go-template={{.data.DATABASE_URL | base64decode}}",
    ])?;
    let ip = kubectl(&["-n", namespace, "get", "svc", KUBE_SERVICE, "-o", "jsonpath={.spec.clusterIP}"])?;
    Ok(url.replace(&format!("{KUBE_SERVICE}.{namespace}.svc.cluster.local"), &ip))
}

fn parse_size(size: &str) -> Result<(u16, u16)> {
    let (columns, rows) = size
        .split_once('x')
        .with_context(|| format!("--size must look like 140x45, got {}", size))?;
    Ok((columns.parse()?, rows.parse()?))
}

/// Apply a scripted key sequence, as given to --keys
fn press_keys(app: &mut App, keys: &str) -> Result<()> {
    for name in keys.split(',').map(str::trim).filter(|k| !k.is_empty()) {
        let code = match name {
            "enter" => KeyCode::Enter,
            "esc" => KeyCode::Esc,
            "tab" => KeyCode::Tab,
            "space" => KeyCode::Char(' '),
            "backspace" => KeyCode::Backspace,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            other => {
                let mut chars = other.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => KeyCode::Char(c),
                    _ => bail!("Unknown key '{}' in --keys", other),
                }
            }
        };
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    }
    Ok(())
}

/// Render a single frame to text, for checking layouts without a terminal
fn dump(app: &App, size: &str) -> Result<()> {
    let (columns, rows) = parse_size(size)?;
    let mut terminal = Terminal::new(TestBackend::new(columns, rows))?;
    terminal.draw(|frame| ui::draw(frame, app))?;
    let buffer = terminal.backend().buffer();
    for y in 0..rows {
        let line: String = (0..columns).map(|x| buffer[(x, y)].symbol()).collect();
        println!("{}", line.trim_end());
    }
    Ok(())
}

/// List detected recurring payments, for judging the detection
fn print_streams(data: &data::Data) {
    let today = data.today();
    let through = forecast::month_end(today);
    println!("{:<22} {:<30} {:<14} {:>4} {:<8} {:>12}  {}", "Account", "Merchant", "Cadence", "N", "Last", "Amount", "Due this month");
    let mut streams: Vec<&forecast::Stream> = data.streams.iter().collect();
    streams.sort_by(|a, b| data.account_name(&a.account_id).cmp(data.account_name(&b.account_id)).then(a.merchant.cmp(&b.merchant)));
    for stream in streams {
        let due: Vec<String> = stream
            .upcoming(today, through)
            .iter()
            .map(|u| format!("{}{}", fmt::short_date(u.date), if u.late { " (late)" } else { "" }))
            .collect();
        println!(
            "{:<22} {:<30} {:<14} {:>4} {:<8} {:>12}  {}",
            fmt::trunc(data.account_name(&stream.account_id), 21),
            fmt::trunc(&stream.merchant, 29),
            stream.cadence.label(),
            stream.payments,
            fmt::short_date(stream.last_date),
            format!("{}{}", if stream.variable { "~" } else { "" }, fmt::money(stream.amount)),
            due.join(", ")
        );
    }
}

/// Run the interface until quit. `load` reads the data afresh, for a reload.
fn run(app: &mut App, load: &dyn Fn() -> Result<data::Data>) -> Result<()> {
    let mut terminal = ratatui::init();
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(std::io::stdout(), DisableMouseCapture);
        previous_hook(info);
    }));
    let mut mouse_capture = false;
    let mut edit_task: Option<std::sync::mpsc::Receiver<Result<edits::Outcome, String>>> = None;
    let result = (|| -> Result<()> {
        execute!(std::io::stdout(), EnableMouseCapture)?;
        mouse_capture = true;
        while !app.quit {
            if let Some(job) = app.edit_job.take() {
                let (sender, receiver) = std::sync::mpsc::channel();
                std::thread::spawn(move || { let _ = sender.send(edits::apply(job)); });
                edit_task = Some(receiver);
            }
            if let Some(task) = &edit_task {
                let completed = match task.try_recv() {
                    Ok(result) => Some(result),
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(Err("Update worker stopped; reload before retrying".into())),
                    Err(std::sync::mpsc::TryRecvError::Empty) => None,
                };
                if let Some(result) = completed {
                    app.edit_busy = false;
                    let message = match result {
                        Ok(outcome) => {
                            let updated = outcome.results.iter().filter(|r| r.updated).count();
                            for row in &outcome.results { if row.updated { app.marked.remove(&row.id); } }
                            let error = outcome.results.iter().find_map(|r| r.error.as_deref());
                            format!("{updated}/{} updated in Monarch{}", outcome.results.len(), error.map(|e| format!("; {e}")).unwrap_or_default())
                        }
                        Err(error) => error,
                    };
                    match load() {
                        Ok(data) => { app.replace_data(data); app.status = message; }
                        Err(_) => app.status = format!("{message}; database reload failed"),
                    }
                    edit_task = None;
                }
            }
            terminal.draw(|frame| ui::draw(frame, app))?;

            if app.reload_requested {
                app.reload_requested = false;
                match load() {
                    Ok(data) => {
                        app.replace_data(data);
                        app.status = "Reloaded".to_string();
                    }
                    Err(e) => app.status = format!("Reload failed: {:#}", e),
                }
                continue;
            }

            if event::poll(if edit_task.is_some() { Duration::from_millis(100) } else { TICK })? {
                match event::read()? {
                    Event::Mouse(mouse) => app.handle_mouse(mouse),
                    Event::Key(key) if key.kind == KeyEventKind::Press => {
                        app.status.clear();
                        app.handle_key(key);
                        if app.reload_requested {
                            app.status = "Reloading…".to_string();
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    })();
    let cleanup = if mouse_capture {
        execute!(std::io::stdout(), DisableMouseCapture)
    } else { Ok(()) };
    ratatui::restore();
    result.and(cleanup.map_err(Into::into))
}

fn main() -> Result<()> {
    let args = Args::parse();
    let Some(screen) = Screen::from_slug(&args.screen) else {
        bail!(
            "Unknown screen '{}'. Choose one of: {}",
            args.screen,
            Screen::ALL.map(Screen::slug).join(", ")
        );
    };

    let load: Box<dyn Fn() -> Result<data::Data>> = if args.demo {
        Box::new(|| Ok(demo::data(chrono::Utc::now().date_naive())))
    } else {
        let database_url = match args.database {
            Some(url) => url,
            None => discover_database_url(&args.namespace).context(
                "No database given. Pass --database or set DATABASE_URL (or try --demo); looking it up with kubectl failed",
            )?,
        };
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        let pool = runtime.block_on(data::connect(&database_url))?;
        Box::new(move || runtime.block_on(data::load(&pool)))
    };

    let mut app = App::new(load()?);
    app.screen = screen;
    if args.anonymize {
        app.toggle_anonymized();
    }
    if args.streams {
        print_streams(&app.data);
        return Ok(());
    }
    if args.dump {
        press_keys(&mut app, &args.keys)?;
        return dump(&app, &args.size);
    }
    run(&mut app, &*load)
}
