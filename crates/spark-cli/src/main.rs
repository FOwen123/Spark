use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "spark", version, about = "Spark: a local-first design tool for working with AI agents")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the Spark server headless and serve the web UI.
    Serve {
        #[arg(long, default_value_t = 29890)]
        port: u16,
        /// Directory holding the built web UI (defaults to web/dist).
        #[arg(long)]
        web_dir: Option<PathBuf>,
    },
    /// Export from a .spark file: images, SVG, PDF, code, tokens, or the whole document as JSON.
    Export {
        file: PathBuf,
        /// Node ids to export (repeatable). Defaults to every artboard on the first page.
        #[arg(long = "node")]
        nodes: Vec<String>,
        /// png, jpg, webp, svg, pdf, html, jsx or tailwind.
        #[arg(long, default_value = "png")]
        format: String,
        /// 1x–4x, or a target size like 512w / 512h.
        #[arg(long, default_value = "1x")]
        scale: String,
        /// Put all nodes in one PDF, a page each.
        #[arg(long)]
        combined: bool,
        /// Print the design tokens as CSS: `theme` (Tailwind @theme) or `root` (:root).
        #[arg(long)]
        tokens: Option<String>,
        /// Dump the whole document as a readable JSON tree.
        #[arg(long)]
        json: bool,
        /// Output file (--json, --tokens) or folder (everything else; defaults to the current folder).
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Run the MCP stdio relay that agents launch (forwards to the running Spark).
    Mcp {
        /// Start a headless Spark server in this process when none is running.
        #[arg(long)]
        headless: bool,
    },
    /// Add Spark to an agent's MCP configuration.
    Connect {
        /// claude-code, codex, cursor, vscode, copilot, opencode, gemini, antigravity or claude-desktop.
        agent: String,
        /// Show what would change without writing.
        #[arg(long)]
        dry_run: bool,
        /// Home directory to edit configs in (defaults to yours; useful for testing).
        #[arg(long)]
        home: Option<PathBuf>,
    },
    /// Remove Spark from an agent's MCP configuration.
    Disconnect {
        agent: String,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        home: Option<PathBuf>,
    },
    /// Write a generated demo file (for screenshots, tests and benchmarks).
    Demo {
        path: PathBuf,
        #[arg(long, default_value_t = 2000)]
        nodes: usize,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Serve { port, web_dir } => {
            let web_dir =
                web_dir.or_else(|| Some(PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../web/dist"))));
            let config = spark_server::Config::load(
                spark_server::config::default_dir(),
                spark_server::config::default_files_dir(),
            )?;
            spark_server::serve(spark_server::ServerOptions { port, web_dir, config }).await
        }
        Command::Export { file, nodes, format, scale, combined, tokens, json, out } => {
            export(file, nodes, &format, &scale, combined, tokens, json, out).await
        }
        Command::Mcp { headless } => {
            let config = spark_server::Config::load(
                spark_server::config::default_dir(),
                spark_server::config::default_files_dir(),
            )?;
            spark_server::relay::run(config, headless, Some(default_web_dir())).await
        }
        Command::Connect { agent, dry_run, home } => connect(&agent, dry_run, home, false),
        Command::Disconnect { agent, dry_run, home } => connect(&agent, dry_run, home, true),
        Command::Demo { path, nodes } => {
            let mut doc = spark_core::demo::demo_document(nodes);
            // Save layout with the file, as Spark does for files it has opened.
            let page = doc.first_page().unwrap();
            for board in doc.children(spark_core::Parent::Page(page)).collect::<Vec<_>>() {
                spark_server::layout::ensure(&mut doc, board);
            }
            spark_core::store::Store::create(&path, &mut doc)?;
            println!("wrote {} ({} nodes)", path.display(), doc.node_count());
            Ok(())
        }
    }
}

/// The built web UI: `SPARK_WEB_DIR`, else the repo's `web/dist` (development builds).
fn default_web_dir() -> PathBuf {
    std::env::var_os("SPARK_WEB_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../web/dist")))
}

fn connect(agent: &str, dry_run: bool, home: Option<PathBuf>, remove: bool) -> anyhow::Result<()> {
    let home = home
        .or_else(|| std::env::var_os("SPARK_HOME_DIR").map(PathBuf::from))
        .or_else(dirs::home_dir)
        .ok_or_else(|| anyhow::anyhow!("can't find your home directory"))?;
    let config_dir = spark_server::config::default_dir();
    let launch = spark_server::connect::Launch::current();
    let c = spark_server::connect::apply(agent, &home, &config_dir, &launch, remove, dry_run)
        .map_err(anyhow::Error::msg)?;
    let verb = if remove { "Removed Spark from" } else { "Added Spark to" };
    if !c.changed {
        println!(
            "{} already {} — nothing to change.",
            c.path.display(),
            if remove { "has no Spark entry" } else { "has Spark" }
        );
    } else if dry_run {
        println!("Would edit {}:\n{}", c.path.display(), c.diff);
    } else {
        println!("{verb} {}:\n{}", c.path.display(), c.diff);
    }
    if let Some(note) = c.note {
        println!("{note}");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn export(
    file: PathBuf,
    nodes: Vec<String>,
    format: &str,
    scale: &str,
    combined: bool,
    tokens: Option<String>,
    json: bool,
    out: Option<PathBuf>,
) -> anyhow::Result<()> {
    use spark_server::export as ex;
    let (store, mut doc) = spark_core::store::Store::open(&file)?;
    let text_out = |text: String| -> anyhow::Result<()> {
        match &out {
            Some(path) => std::fs::write(path, text)?,
            // Ignore broken pipes (`spark export --json | head`).
            None => drop(std::io::Write::write_all(&mut std::io::stdout(), text.as_bytes())),
        }
        Ok(())
    };
    if json {
        return text_out(serde_json::to_string_pretty(&spark_core::export::to_json(&doc))? + "\n");
    }
    if let Some(kind) = tokens {
        anyhow::ensure!(matches!(kind.as_str(), "theme" | "root"), "--tokens takes theme or root");
        return text_out(spark_core::tokens::to_css(&doc.tokens, kind == "theme"));
    }
    let ids: Vec<spark_core::NodeId> = if nodes.is_empty() {
        let page = doc.first_page().ok_or_else(|| anyhow::anyhow!("the file has no pages"))?;
        doc.children(spark_core::Parent::Page(page)).filter(|id| doc.node(*id).is_some()).collect()
    } else {
        nodes
            .iter()
            .map(|n| n.parse().map_err(|_| anyhow::anyhow!("`{n}` is not a node id")))
            .collect::<anyhow::Result<_>>()?
    };
    for id in &ids {
        anyhow::ensure!(doc.node(*id).is_some(), "node {id} not found");
    }
    let dir = out.unwrap_or_else(|| PathBuf::from("."));
    if combined {
        let bytes = ex::combined_pdf(&mut doc, &ids).map_err(anyhow::Error::msg)?;
        say(&ex::save(&dir, &doc.name, "pdf", &bytes).map_err(anyhow::Error::msg)?.display().to_string());
        return Ok(());
    }
    let presets = vec![(
        ex::Format::parse(format).map_err(anyhow::Error::msg)?,
        ex::Scale::parse(scale).map_err(anyhow::Error::msg)?,
        String::new(),
        90,
    )];
    let path = store.path().to_path_buf();
    let open = std::sync::Arc::new(std::sync::Mutex::new(spark_server::hub::OpenDoc {
        editor: spark_core::history::Editor::new(doc),
        store,
        path,
        subscribers: Default::default(),
        gesture: None,
        last_write: std::time::Instant::now(),
        save_error: None,
    }));
    for id in ids {
        for p in ex::export_presets(None, &open, id, &presets, &dir).await.map_err(anyhow::Error::msg)? {
            say(&p.display().to_string());
        }
    }
    Ok(())
}

/// Prints a line, ignoring a closed pipe (`spark export … | head`).
fn say(line: &str) {
    let _ = std::io::Write::write_all(&mut std::io::stdout(), format!("{line}\n").as_bytes());
}
