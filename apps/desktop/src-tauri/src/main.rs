// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;

use spark_server::Hub;
use tauri::{
    AppHandle, Emitter, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder,
    menu::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu},
    utils::config::BackgroundThrottlingPolicy,
};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_dialog::DialogExt;

/// Opens files and `spark://` links passed on the command line or by the OS.
fn open_args(hub: &Hub, args: impl IntoIterator<Item = String>) {
    for a in args {
        if !(a.starts_with("spark://") || a.ends_with(".spark") || a.starts_with("file://")) {
            continue;
        }
        if let Err(e) = hub.reveal(&a) {
            eprintln!("spark: can't open {a}: {e}");
        }
    }
}

fn focus(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Native menus. Spark actions travel to the UI as commands; Cut, Copy, Paste and Select All
/// stay native so text fields and the canvas clipboard both work.
fn menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let item = |id: &str, label: &str, accel: Option<&str>| MenuItem::with_id(app, id, label, true, accel);
    let file = Submenu::with_items(
        app,
        "File",
        true,
        &[
            &item("newFile", "New File", Some("CmdOrCtrl+N"))?,
            &item("openFile", "Open…", Some("CmdOrCtrl+O"))?,
            &PredefinedMenuItem::separator(app)?,
            &item("export", "Export…", Some("CmdOrCtrl+Shift+E"))?,
            &PredefinedMenuItem::separator(app)?,
            &item("closeTab", "Close Tab", Some("CmdOrCtrl+W"))?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &item("undo", "Undo", Some("CmdOrCtrl+Z"))?,
            &item("redo", "Redo", Some("CmdOrCtrl+Shift+Z"))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &item("duplicate", "Duplicate", Some("CmdOrCtrl+D"))?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;
    let view = Submenu::with_items(
        app,
        "View",
        true,
        &[
            &item("zoomIn", "Zoom In", Some("CmdOrCtrl+="))?,
            &item("zoomOut", "Zoom Out", Some("CmdOrCtrl+-"))?,
            &item("zoom100", "Actual Size", Some("CmdOrCtrl+0"))?,
            &item("zoomFit", "Zoom to Fit", None)?,
            &PredefinedMenuItem::separator(app)?,
            &item("panel.agent", "Agent Panel", Some("CmdOrCtrl+J"))?,
            &PredefinedMenuItem::fullscreen(app, None)?,
        ],
    )?;
    let window = Submenu::with_items(
        app,
        "Window",
        true,
        &[&PredefinedMenuItem::minimize(app, None)?, &PredefinedMenuItem::maximize(app, None)?],
    )?;
    let mut menus: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = Vec::new();
    #[cfg(target_os = "macos")]
    let app_menu = Submenu::with_items(
        app,
        "Spark",
        true,
        &[
            &PredefinedMenuItem::about(
                app,
                None,
                Some(AboutMetadata { name: Some("Spark".into()), ..Default::default() }),
            )?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, None)?,
        ],
    )?;
    #[cfg(target_os = "macos")]
    menus.push(&app_menu);
    #[cfg(not(target_os = "macos"))]
    let _ = AboutMetadata::default();
    menus.extend([&file as &dyn tauri::menu::IsMenuItem<tauri::Wry>, &edit, &view, &window]);
    Menu::with_items(app, &menus)
}

fn main() {
    // Agents launch this same binary as their MCP stdio relay (`<app> mcp`).
    if std::env::args().nth(1).as_deref() == Some("mcp") {
        let headless = std::env::args().any(|a| a == "--headless");
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let result = runtime.block_on(async {
            let config = spark_server::Config::load(
                spark_server::config::default_dir(),
                spark_server::config::default_files_dir(),
            )?;
            spark_server::relay::run(config, headless, None).await
        });
        if let Err(e) = result {
            eprintln!("spark mcp: {e}");
            std::process::exit(1);
        }
        return;
    }
    let app = tauri::Builder::default()
        // Must come first: a second launch hands its files and links to the running app.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if let Some(hub) = app.try_state::<Arc<Hub>>() {
                open_args(&hub, argv.into_iter().skip(1));
            }
            focus(app);
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .setup(|app| {
            let bundled = app.path().resource_dir().ok().map(|d| d.join("web")).filter(|d| d.exists());
            // `tauri dev` has no bundled resources; serve the repo's web build instead.
            let web_dir = bundled.or_else(|| {
                cfg!(debug_assertions)
                    .then(|| std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../web/dist")))
            });
            let config = spark_server::Config::load(
                spark_server::config::default_dir(),
                spark_server::config::default_files_dir(),
            )?;
            let token = config.token.clone();
            let listener = tauri::async_runtime::block_on(spark_server::bind(29890))?;
            let port = listener.local_addr()?.port();
            let (hub, server) =
                tauri::async_runtime::block_on(async { spark_server::prepare(listener, web_dir, config) })?;
            tauri::async_runtime::spawn(async move {
                if let Err(e) = server.await {
                    eprintln!("spark server stopped: {e}");
                }
            });
            app.manage(hub.clone());

            app.set_menu(menu(app.handle())?)?;
            let open_dialog = {
                let h = hub.clone();
                let handle = app.handle().clone();
                move || {
                    let h = h.clone();
                    handle.dialog().file().add_filter("Spark design", &["spark"]).pick_file(move |path| {
                        if let Some(p) = path.and_then(|p| p.into_path().ok()) {
                            let _ = h.reveal(&p.to_string_lossy());
                        }
                    });
                }
            };
            let dialog = open_dialog.clone();
            let _ = hub.native.set(Box::new(move |cmd| {
                if cmd == "openFile" {
                    dialog();
                }
            }));
            let h = hub.clone();
            app.on_menu_event(move |_, event| match event.id().as_ref() {
                "newFile" => match h.create("Untitled") {
                    Ok(d) => {
                        let id = d.lock().unwrap().id().to_string();
                        let _ = h.reveal(&id);
                    }
                    Err(e) => eprintln!("spark: {e}"),
                },
                "openFile" => open_dialog(),
                other => h.menu(other),
            });

            // Links that launched the app, and links while it runs.
            let h = hub.clone();
            app.deep_link().on_open_url(move |event| open_args(&h, event.urls().into_iter().map(|u| u.to_string())));
            #[cfg(any(windows, target_os = "linux"))]
            let _ = app.deep_link().register_all();

            // The UI learns it's in the desktop app (native menus own some shortcuts) from `?desktop`.
            let url = format!("http://127.0.0.1:{port}/?desktop=1#token={token}").parse()?;
            let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
                .title("Spark")
                .inner_size(1440.0, 900.0)
                .min_inner_size(900.0, 600.0)
                // Agents work in documents while Spark is in the background; keep the page live.
                .background_throttling(BackgroundThrottlingPolicy::Disabled);
            // On macOS the app bar doubles as the title bar, with the window buttons inside its 40px row.
            // Tauri's y sets the height of the strip holding the buttons, not their top offset; 22 centers them.
            #[cfg(target_os = "macos")]
            let window = window
                .title_bar_style(tauri::TitleBarStyle::Overlay)
                .hidden_title(true)
                .traffic_light_position(tauri::LogicalPosition::new(13.0, 22.0));
            window.build()?;
            let start: Vec<String> = std::env::args().skip(1).collect();
            let h = hub.clone();
            tauri::async_runtime::spawn(async move {
                // Give the UI a moment to connect, then open files passed on the command line.
                tokio::time::sleep(std::time::Duration::from_millis(800)).await;
                open_args(&h, start);
            });
            let _ = app.emit("spark-ready", ());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Spark");
    app.run(|app, event| {
        // macOS delivers double-clicked .spark files here.
        #[cfg(target_os = "macos")]
        if let RunEvent::Opened { urls } = event {
            if let Some(hub) = app.try_state::<Arc<Hub>>() {
                open_args(&hub, urls.iter().map(|u| u.to_string()));
            }
            focus(app);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (app, event);
    });
}
