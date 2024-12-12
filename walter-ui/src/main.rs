mod app;
mod ui;
mod utils;

use app::{App, CurrentScreen};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::{CrosstermBackend, Terminal},
    widgets::ScrollbarState,
};
use std::{
    error::Error,
    io::{self, Stdout},
};
use ui::render_ui;
use walter_core::client::upload_blob;
use walter_core::migrator::migrate_files;
use walter_core::updater;
use walter_db;

use clipboard::{ClipboardContext, ClipboardProvider};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        match args[1].as_str() {
            "--help" | "-h" => {
                println!("\x1b[1;32mUsage:\x1b[0m walter-ui --cli [OPTIONS]");
                println!("\x1b[1;32mOptions:\x1b[0m");
                println!("  \x1b[1;32m-h, --help\x1b[0m    Print this help message");
                println!("  \x1b[1;32m-c, --cli\x1b[0m     CLI mode");
                println!("  \x1b[1;32m-s, --setup\x1b[0m   Setup Walter along with Walrus CLI, Walrus Site Builder and Sui Client");
                println!("  \x1b[1;32m-u, --update\x1b[0m  Update Walter and its dependencies");
                println!("  \x1b[1;32m-sql, --sqlite\x1b[0m Run Walrus SQLite shell with rollbacks. Requires blobID as argument");
                println!("  \x1b[1;32m-bs, --build-site\x1b[0m Build Walrus site with the given app path");
                println!(
                    "  \x1b[1;32m-rp, --run-pinner\x1b[0m Run Walrus Pinner in the background"
                );
                return Ok(());
            }
            "--tui" | "-c" => {
                println!("\x1b[1;32mWalTerminalUI\x1b[0m - \x1b[1;34mA TUI Devtool keychain for Walrus\x1b[0m");
                println!("\x1b[1;32mVersion:\x1b[0m 0.1.0");
            }
            "--setup" | "-s" => {
                std::env::set_current_dir("..").expect("Failed to change directory.");
                println!("Running setup...");
                let output = std::process::Command::new("make")
                    .arg("all")
                    .output()
                    .expect("Failed to execute make command");

                if output.status.success() {
                    println!("Setup completed successfully.");
                } else {
                    eprintln!(
                        "Setup failed with error: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
                return Ok(());
            }
            "--update" | "-u" => {
                updater::run();
            }
            "--sqlite" | "-sql" => {
                walter_db::main().unwrap();
                return Ok(());
            }
            "--build-site" | "-bs" => {
                let app_path = args[2].as_str();
                let output = std::process::Command::new("site-builder")
                    .arg("publish")
                    .arg(app_path)
                    .output()
                    .expect("Failed to execute command.");

                if output.status.success() {
                    println!("Site built successfully.");
                } else {
                    eprintln!(
                        "Site build failed with error: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
                return Ok(());
            }
            "--run-pinner" | "-rp" => {
                std::env::set_current_dir("../wal-pinner").expect("Failed to change directory.");

                let output = std::process::Command::new("npm")
                    .arg("install")
                    .output()
                    .expect("Failed to execute npm install command.");

                if output.status.success() {
                    println!("npm install completed successfully.");
                } else {
                    eprintln!(
                        "npm install failed with error: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }

                println!("Starting pinner in the background...");

                std::process::Command::new("npx")
                    .arg("ts-node")
                    .arg("../wal-pinner/src/index.ts")
                    .spawn()
                    .expect("Failed to start pinner process in the background.");
                return Ok(());
            }
            _ => {
                eprintln!("Unknown option: {}", args[1]);
                return Ok(());
            }
        }
    }
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();

    let sui_active_env = utils::sui_active_env().await?;
    let sui_active_env = sui_active_env.trim().to_string();
    app.sui_active_env = sui_active_env;

    let sui_active_address = utils::sui_active_address().await?;
    let sui_active_address = sui_active_address.trim().to_string();
    app.sui_active_address = sui_active_address;

    let user_blobs = utils::walrus_list_blobs().await?;
    let user_blobs = serde_json::from_str(&user_blobs)?;
    app.user_blobs = user_blobs;

    let walrus_system_info = utils::walrus_info_system().await?;
    let walrus_system_info = walrus_system_info.trim().to_string();
    app.walrus_system_info = walrus_system_info;

    let _res = run_app(&mut terminal, &mut app).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
) -> io::Result<bool> {
    loop {
        terminal.draw(|frame| render_ui(frame, app))?;

        if let Event::Key(key) = event::read()? {
            if key.kind == event::KeyEventKind::Press {
                if key.code == KeyCode::Esc {
                    if app.is_editing {
                        app.is_editing = false;
                    }
                }
                if key.code == KeyCode::Char('e') || key.code == KeyCode::Char('E') {
                    if !app.is_editing {
                        app.is_editing = true;
                        continue;
                    }
                }

                if !app.should_quit && !app.is_editing {
                    match key.code {
                        KeyCode::Char('q') => app.should_quit = true,
                        _ => {}
                    }
                }

                if app.should_quit {
                    match key.code {
                        KeyCode::Char('y') => return Ok(true),
                        KeyCode::Char('n') => app.should_quit = false,
                        _ => {}
                    }
                }

                if key.code == KeyCode::Char('1') {
                    app.current_screen = CurrentScreen::Dashboard;
                    app.is_editing = false;
                }

                if key.code == KeyCode::Char('2') {
                    app.current_screen = CurrentScreen::Uploader;
                    app.is_editing = false;
                }

                if key.code == KeyCode::Char('3') {
                    app.current_screen = CurrentScreen::Migrator;
                    app.is_editing = false;
                }

                if key.code == KeyCode::Char('4') {
                    app.current_screen = CurrentScreen::SharderAndEpochExtender;
                    app.is_editing = false;
                }
            }

            match app.current_screen {
                CurrentScreen::Splash => match key.code {
                    KeyCode::Enter => {
                        app.current_screen = CurrentScreen::Dashboard;
                        if !&app.user_blobs.is_empty() {
                            app.scrollbar_state = ScrollbarState::new(&app.user_blobs.len() - 1);
                        }
                    }
                    _ => {}
                },
                CurrentScreen::Dashboard => match key.code {
                    KeyCode::Char('c') | KeyCode::Char('C') => {
                        let mut ctx: ClipboardContext = ClipboardProvider::new().unwrap();
                        let selected_index = app.table_state.selected().unwrap_or(0);
                        ctx.set_contents(app.user_blobs[selected_index].blob_id.clone())
                            .unwrap();
                    }
                    KeyCode::Up => {
                        app.prev_row();
                    }
                    KeyCode::Down => {
                        app.next_row();
                    }
                    _ => {}
                },
                CurrentScreen::Uploader => match key.code {
                    KeyCode::Up | KeyCode::Char('+') => {
                        if app.epochs < 200 {
                            app.epochs += 1;
                        }
                    }
                    KeyCode::Down | KeyCode::Char('-') => {
                        if app.epochs > 1 {
                            app.epochs -= 1;
                        }
                    }
                    KeyCode::Char(value) => {
                        if app.is_editing {
                            app.filename += &value.to_string();
                        }
                    }
                    KeyCode::Backspace => {
                        if app.is_editing {
                            if app.filename.len() > 0 {
                                app.filename.pop();
                            }
                        }
                    }
                    KeyCode::Enter => {
                        app.migration_status = "Uploading file...".into();
                        terminal.draw(|frame| render_ui(frame, app))?;

                        let res = upload_blob(&app.filename, app.epochs).await;
                        match res {
                            Ok(blob_id) => {
                                app.file_upload_status =
                                    format!("File uploaded successfully! Blob ID {}", blob_id);
                                let user_blobs = utils::walrus_list_blobs().await.unwrap();
                                let user_blobs = serde_json::from_str(&user_blobs).unwrap();
                                app.user_blobs = user_blobs;
                            }
                            Err(e) => {
                                app.file_upload_status =
                                    format!("File upload Failed: {e}").to_string();
                            }
                        }
                    }
                    _ => {}
                },
                CurrentScreen::Migrator => match key.code {
                    KeyCode::Char('P') | KeyCode::Char('p') => {
                        let mut ctx: clipboard::ClipboardContext =
                            ClipboardProvider::new().unwrap();
                        app.pinata_api_key = ctx.get_contents().unwrap();
                    }
                    KeyCode::Char('x') => {
                        app.pinata_api_key = "".into();
                    }
                    KeyCode::Char('M') | KeyCode::Char('m') => {
                        app.migration_status = "Migrating files...".into();
                        terminal.draw(|frame| render_ui(frame, app))?;

                        let res = migrate_files(&app.pinata_api_key).await;
                        match res {
                            Ok(_) => {
                                app.migration_status = "Migration successful".into();
                                let user_blobs = utils::walrus_list_blobs().await.unwrap();
                                let user_blobs = serde_json::from_str(&user_blobs).unwrap();
                                app.user_blobs = user_blobs;
                            }
                            Err(e) => {
                                app.migration_status = format!("Migration failed: {}", e);
                            }
                        }
                    }
                    _ => {}
                },
                CurrentScreen::SharderAndEpochExtender => match key.code {
                    KeyCode::Char('K') | KeyCode::Char('k') => {
                        if app.is_editing {
                            app.filename += &key.code.to_string();
                        } else {
                            app.sharder_status = "Sharding Started...".into();
                            terminal.draw(|frame| render_ui(frame, app))?;

                            let status = app.upload_shard().await;
                            app.sharder_status = status;
                        }
                    }
                    KeyCode::Char('Y') | KeyCode::Char('y') => {
                        if app.is_editing {
                            app.filename += &key.code.to_string();
                        } else {
                            app.shard_pass = "password".into();
                            let status = app.upload_shard().await;
                            app.sharder_status = status;
                        }
                    }
                    KeyCode::Char('T') | KeyCode::Char('t') => {
                        if app.is_editing {
                            app.filename += &key.code.to_string();
                        } else {
                            let status = app.extend_blob_epoch().await;
                            app.extender_status = status;
                        }
                    }
                    KeyCode::Char('P') | KeyCode::Char('p') => {
                        if app.is_editing {
                            app.filename += &key.code.to_string();
                        } else {
                            let mut ctx: clipboard::ClipboardContext =
                                ClipboardProvider::new().unwrap();
                            app.extender_blob_id = ctx.get_contents().unwrap();
                        }
                    }
                    KeyCode::Char(value) => {
                        if app.is_editing {
                            app.filename += &value.to_string();
                        }
                    }
                    KeyCode::Backspace => {
                        if app.is_editing {
                            if app.filename.len() > 0 {
                                app.filename.pop();
                            }
                        }
                    }
                    _ => {}
                },
            }
        }
    }
}
