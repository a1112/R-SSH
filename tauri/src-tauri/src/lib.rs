use std::{path::PathBuf, sync::Mutex};

use rssh_web::{WebServer, WebServerConfig};
#[cfg(not(debug_assertions))]
use tauri::path::BaseDirectory;
use tauri::{Manager, RunEvent, Url};
use tokio::sync::oneshot;

struct BackendState {
    shutdown: Mutex<Option<oneshot::Sender<()>>>,
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let web_root = web_root(app.handle())?;
            let server = tauri::async_runtime::block_on(WebServer::bind(WebServerConfig {
                listen: "127.0.0.1:0".parse().expect("loopback endpoint is valid"),
                web_root,
                max_sessions: rssh_web::server::DEFAULT_MAX_SESSIONS,
                allowed_origin: None,
            }))?;
            let bootstrap_url = server.bootstrap_url();
            let (shutdown_tx, shutdown_rx) = oneshot::channel();

            tauri::async_runtime::spawn(async move {
                if let Err(error) = server
                    .run_until(async move {
                        let _ = shutdown_rx.await;
                    })
                    .await
                {
                    eprintln!("R-SSH Web backend stopped: {error}");
                }
            });
            app.manage(BackendState {
                shutdown: Mutex::new(Some(shutdown_tx)),
            });

            let window = app
                .get_webview_window("main")
                .ok_or_else(|| "main Tauri window is missing".to_owned())?;
            window.navigate(
                bootstrap_url
                    .parse::<Url>()
                    .map_err(|error| format!("invalid backend URL: {error}"))?,
            )?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building R-SSH Tauri application")
        .run(|app_handle, event| {
            if !matches!(event, RunEvent::ExitRequested { .. }) {
                return;
            }
            if let Some(state) = app_handle.try_state::<BackendState>()
                && let Ok(mut shutdown) = state.shutdown.lock()
                && let Some(sender) = shutdown.take()
            {
                let _ = sender.send(());
            }
        });
}

fn web_root(_app: &tauri::AppHandle) -> Result<PathBuf, Box<dyn std::error::Error>> {
    #[cfg(debug_assertions)]
    {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web/dist");
        if !root.join("index.html").is_file() {
            return Err(format!(
                "web assets are missing at {}; run `cd web && npm run build` first",
                root.display()
            )
            .into());
        }
        Ok(root)
    }

    #[cfg(not(debug_assertions))]
    {
        Ok(_app.path().resolve("web/dist", BaseDirectory::Resource)?)
    }
}
