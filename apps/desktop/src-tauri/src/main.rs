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
use tauri_plugin_dialog::{DialogExt, MessageDialogBuilder, MessageDialogButtons};
use tauri_plugin_updater::UpdaterExt;

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

/// An update dialog shown as a sheet on the main window, so it waits with Spark instead of
/// floating as a separate system alert.
fn update_dialog(app: &AppHandle, text: String) -> Option<MessageDialogBuilder<tauri::Wry>> {
    let window = app.get_webview_window("main")?;
    Some(app.dialog().message(text).title("Software Update").parent(&window))
}

/// Looks for a newer published release and offers to install it. The launch check (`quiet`) only
/// speaks up when there is one; the menu item also says when Spark is current or the check failed.
async fn check_for_update(app: AppHandle, quiet: bool) {
    let say = |text: String| {
        if let Some(d) = update_dialog(&app, text) {
            d.show(|_| {});
        }
    };
    let update = match app.updater() {
        Ok(updater) => updater.check().await,
        Err(e) => Err(e),
    };
    let update = match update {
        Ok(Some(update)) => update,
        Ok(None) if !quiet => return say(format!("Spark {} is the latest version.", app.package_info().version)),
        Err(e) if !quiet => return say(format!("Couldn't check for updates: {e}")),
        _ => return,
    };
    let text = format!(
        "Spark {} is available (you have {}). Your files are already saved.",
        update.version, update.current_version
    );
    let Some(ask) = update_dialog(&app, text) else { return };
    let handle = app.clone();
    ask.buttons(MessageDialogButtons::OkCancelCustom("Install and Restart".into(), "Later".into())).show(
        move |install| {
            if !install {
                return;
            }
            tauri::async_runtime::spawn(async move {
                match update.download_and_install(|_, _| {}, || {}).await {
                    Ok(()) => handle.restart(),
                    Err(e) => {
                        if let Some(d) = update_dialog(&handle, format!("Couldn't install the update: {e}")) {
                            d.show(|_| {});
                        }
                    }
                }
            });
        },
    );
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
            &item("pasteOnTop", "Paste on Top", None)?,
            &item("duplicate", "Duplicate", Some("CmdOrCtrl+D"))?,
            &PredefinedMenuItem::select_all(app, None)?,
            &item("selectOthers", "Select Others", None)?,
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
            &item("toggleUI", "Show/Hide UI", None)?,
            &PredefinedMenuItem::separator(app)?,
            // No accelerators on the tab items: the page binds Ctrl+Tab and Ctrl+Shift+Tab itself.
            &item("tab.next", "Next Tab", None)?,
            &item("tab.prev", "Previous Tab", None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::fullscreen(app, None)?,
        ],
    )?;
    let window = Submenu::with_items(
        app,
        "Window",
        true,
        &[&PredefinedMenuItem::minimize(app, None)?, &PredefinedMenuItem::maximize(app, None)?],
    )?;
    let help = Submenu::with_id_and_items(
        app,
        tauri::menu::HELP_SUBMENU_ID,
        "Help",
        true,
        &[&item("app.shortcuts", "Keyboard Shortcuts", None)?],
    )?;
    #[cfg(target_os = "macos")]
    help.set_as_help_menu_for_nsapp()?;
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
            &item("checkUpdates", "Check for Updates…", None)?,
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
    menus.extend([&file as &dyn tauri::menu::IsMenuItem<tauri::Wry>, &edit, &view, &window, &help]);
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
        .plugin(tauri_plugin_updater::Builder::new().build())
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
            app.on_menu_event(move |app, event| match event.id().as_ref() {
                "newFile" => match h.create("Untitled") {
                    Ok(d) => {
                        let id = d.lock().unwrap().id().to_string();
                        let _ = h.reveal(&id);
                    }
                    Err(e) => eprintln!("spark: {e}"),
                },
                "openFile" => open_dialog(),
                "checkUpdates" => drop(tauri::async_runtime::spawn(check_for_update(app.clone(), false))),
                other => h.menu(other),
            });

            // Links that launched the app, and links while it runs.
            let h = hub.clone();
            app.deep_link().on_open_url(move |event| open_args(&h, event.urls().into_iter().map(|u| u.to_string())));
            #[cfg(any(windows, target_os = "linux"))]
            let _ = app.deep_link().register_all();

            // The UI learns it's in the desktop app (native menus own some shortcuts) from `?desktop`.
            let url = format!("http://127.0.0.1:{port}/?desktop=1#token={token}").parse()?;
            let handle = app.handle().clone();
            let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
                .on_new_window(move |url, features| {
                    use tauri::webview::NewWindowResponse;
                    if url.scheme() != "http"
                        || url.host_str() != Some("127.0.0.1")
                        || url.port() != Some(port)
                        || !url.path().starts_with("/assets/")
                    {
                        return NewWindowResponse::Deny;
                    }
                    let label = format!("image-{}", url.path().trim_start_matches("/assets/"));
                    if let Some(window) = handle.get_webview_window(&label) {
                        let _ = window.set_focus();
                        return NewWindowResponse::Deny;
                    }
                    match WebviewWindowBuilder::new(
                        &handle,
                        label,
                        WebviewUrl::External("about:blank".parse().unwrap()),
                    )
                    .window_features(features)
                    .title("Spark image")
                    .build()
                    {
                        Ok(window) => NewWindowResponse::Create { window },
                        Err(e) => {
                            eprintln!("spark: could not open image: {e}");
                            NewWindowResponse::Deny
                        }
                    }
                })
                // Tabs, layers and file imports use the frontend HTML drag and drop handlers.
                .disable_drag_drop_handler()
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
            // Tests and managed installs set SPARK_DISABLE_UPDATER to skip the launch check.
            if std::env::var_os("SPARK_DISABLE_UPDATER").is_none() {
                tauri::async_runtime::spawn(check_for_update(app.handle().clone(), true));
            }
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
