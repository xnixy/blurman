//! Blurman puts adjustable frosted glass behind the Windows apps you choose.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod autostart;
mod glass;
mod hotkey;
mod ipc;
mod mapping;
mod picker;
mod rules;
mod shared;
mod system;
mod target;
mod theme;
mod tray;
mod update;
mod worker;

use clap::{Parser, Subcommand};
use mapping::{BlurStyle, FocusLook, Look, Rgb, TintSource};
use shared::Shared;
use std::path::PathBuf;
use std::time::Duration;
use windows::Win32::System::Console::{AllocConsole, AttachConsole, ATTACH_PARENT_PROCESS};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

#[derive(Parser)]
#[command(
    name = "blurman",
    about = "Frosted glass for the apps you pick. Run with no command to open the window."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    /// Set by the Windows startup entry: run in the tray without opening the window.
    #[arg(long, hide = true)]
    startup: bool,
    /// Same as BLURMAN_PROFILE. Lets a profile's startup entry come back as that profile.
    #[arg(long, hide = true, global = true)]
    profile: Option<String>,
}

#[derive(Subcommand)]
enum Command {
    /// Print visible apps Blurman can frost.
    List,
    /// Save a rule for an app. Opens Blurman if it is not running. Options you leave out keep
    /// the app's current values, or the preset's.
    Apply {
        process: String,
        /// Start from a saved preset, like "Subtle" or "Heavy glass".
        #[arg(long)]
        preset: Option<String>,
        #[arg(long, value_parser = clap::value_parser!(u8).range(mapping::TRANSPARENCY_MIN as i64..=mapping::TRANSPARENCY_MAX as i64))]
        transparency: Option<u8>,
        #[arg(long, value_parser = clap::value_parser!(u8).range(mapping::BLUR_MIN as i64..=mapping::BLUR_MAX as i64))]
        blur: Option<u8>,
        /// Frost blurs what is behind the window. Acrylic is milky system glass, and the blur value sets how milky it is.
        #[arg(long, value_enum)]
        style: Option<BlurStyle>,
        /// Tint color, like "#1a2b3c".
        #[arg(long, value_parser = parse_color)]
        tint: Option<Rgb>,
        /// Use the tint color given, or follow the Windows theme or accent color.
        #[arg(long, value_enum)]
        tint_from: Option<TintSource>,
        /// How much tint is laid over frost, 0 to 80.
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=mapping::TINT_MAX as i64))]
        tint_amount: Option<u8>,
        /// Color strength of what shows through, 0 to 200. 100 leaves it as it is.
        #[arg(long, value_parser = clap::value_parser!(u8).range(mapping::SATURATION_MIN as i64..=mapping::SATURATION_MAX as i64))]
        saturation: Option<u8>,
        /// Film grain on the glass, 0 to 100.
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=mapping::GRAIN_MAX as i64))]
        grain: Option<u8>,
        /// How the app looks while you use it.
        #[arg(long, value_enum)]
        focus: Option<FocusLook>,
        /// Only frost windows with this in the title. Repeat for more. `*` is a wildcard. "" clears.
        #[arg(long = "only-title")]
        only_titles: Vec<String>,
        /// Leave windows with this in the title alone. Repeat for more. `*` is a wildcard. "" clears.
        #[arg(long = "skip-title")]
        skip_titles: Vec<String>,
    },
    /// Take the glass off one app, or every app with --all.
    Clear {
        #[arg(required_unless_present = "all")]
        process: Option<String>,
        #[arg(long, conflicts_with = "process")]
        all: bool,
    },
    /// Put every app back without deleting rules.
    Pause,
    /// Frost the apps again after a pause.
    Resume,
    /// Pause if frosting, resume if paused.
    Toggle,
    /// Show whether Blurman is running, paused, and what it frosts.
    Status {
        /// Print JSON, for scripts.
        #[arg(long)]
        json: bool,
    },
    /// Write your rules and presets to a file.
    Export { file: PathBuf },
    /// Add rules and presets from a file written by export.
    Import {
        file: PathBuf,
        /// Replace every rule and preset instead of adding to them.
        #[arg(long)]
        replace: bool,
    },
}

fn parse_color(text: &str) -> Result<Rgb, String> {
    Rgb::parse(text).ok_or_else(|| format!("\"{text}\" is not a color like #1a2b3c."))
}

fn main() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            ensure_console();
            err.exit();
        }
    };
    if let Some(profile) = &cli.profile {
        // Before anything reads the profile, while this is the only thread.
        std::env::set_var("BLURMAN_PROFILE", profile);
    }
    let Some(command) = cli.command else {
        launch(cli.startup);
        return;
    };
    ensure_console();
    let result = run_command(command);
    if let Some(note) = rules::take_recovery_note() {
        eprintln!("{note}");
    }
    if let Err(err) = result {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run_command(command: Command) -> Result<(), String> {
    match command {
        Command::List => {
            for group in target::list_groups() {
                println!(
                    "{:<28} {:>2} window{}{}",
                    group.process,
                    group.windows,
                    if group.windows == 1 { " " } else { "s" },
                    if group.elevated {
                        "  administrator"
                    } else {
                        ""
                    }
                );
                if !group.sample_title.is_empty() {
                    println!("  {}", group.sample_title);
                }
            }
        }
        Command::Apply {
            process,
            preset,
            transparency,
            blur,
            style,
            tint,
            tint_from,
            tint_amount,
            saturation,
            grain,
            focus,
            only_titles,
            skip_titles,
        } => {
            if mapping::normalize_process(&process).is_empty() {
                return Err("Give an app name, like chrome.exe.".into());
            }
            let mut store = rules::load();
            let mut rule = store
                .rule(&process)
                .cloned()
                .unwrap_or_else(|| rules::new_rule(&process, Look::default()));
            if let Some(name) = preset {
                let settings = rules::load_settings();
                let found = settings.preset(&name).ok_or_else(|| {
                    let names: Vec<&str> =
                        settings.presets.iter().map(|p| p.name.as_str()).collect();
                    format!(
                        "No preset is called \"{name}\". Try one of: {}.",
                        names.join(", ")
                    )
                })?;
                rule.look = found.look;
            }
            let look = &mut rule.look;
            look.transparency = transparency.unwrap_or(look.transparency);
            look.blur = blur.unwrap_or(look.blur);
            look.style = style.unwrap_or(look.style);
            let finish = &mut look.finish;
            finish.tint = tint.unwrap_or(finish.tint);
            // Naming a color means using it, unless the theme or accent was asked for too.
            finish.tint_source = tint_from.unwrap_or(if tint.is_some() {
                TintSource::Custom
            } else {
                finish.tint_source
            });
            finish.tint_amount = tint_amount.unwrap_or(finish.tint_amount);
            finish.saturation = saturation.unwrap_or(finish.saturation);
            finish.grain = grain.unwrap_or(finish.grain);
            rule.focus = focus.unwrap_or(rule.focus);
            // Given at all, the list replaces the old one. `--skip-title ""` clears it.
            let given = |titles: Vec<String>| -> Option<Vec<String>> {
                (!titles.is_empty()).then(|| {
                    titles
                        .into_iter()
                        .filter(|title| !title.trim().is_empty())
                        .collect()
                })
            };
            if let Some(titles) = given(only_titles) {
                rule.only_titles = titles;
            }
            if let Some(titles) = given(skip_titles) {
                rule.skip_titles = titles;
            }
            rule.enabled = true;
            store.paused = false;
            store.upsert(rule.clone());
            rules::save(&store)?;
            ipc::reload_or_launch()?;
            println!("Frosting {}.", describe(&rule));
        }
        Command::Pause | Command::Resume | Command::Toggle => {
            let mut store = rules::load();
            store.paused = match command {
                Command::Pause => true,
                Command::Resume => false,
                _ => !store.paused,
            };
            rules::save(&store)?;
            ipc::signal(ipc::msg_reload());
            println!("{}", if store.paused { "Paused." } else { "Frosting." });
        }
        Command::Status { json } => {
            let store = rules::load();
            let running = ipc::find_host().is_some();
            if json {
                let status = serde_json::json!({
                    "running": running,
                    "paused": store.paused,
                    "rules": store.rules,
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&status).map_err(|err| err.to_string())?
                );
                return Ok(());
            }
            println!(
                "Blurman is {}{}.",
                if running { "running" } else { "not running" },
                if store.paused { " and paused" } else { "" }
            );
            for rule in &store.rules {
                println!(
                    "  {} {}",
                    if rule.enabled { "on " } else { "off" },
                    describe(rule)
                );
            }
        }
        Command::Export { file } => {
            let bundle = rules::Bundle {
                rules: rules::load().rules,
                presets: rules::load_settings().presets,
            };
            let text = serde_json::to_string_pretty(&bundle).map_err(|err| err.to_string())?;
            std::fs::write(&file, text).map_err(|err| format!("{}: {err}", file.display()))?;
            println!(
                "Wrote {} rules and {} presets to {}.",
                bundle.rules.len(),
                bundle.presets.len(),
                file.display()
            );
        }
        Command::Import { file, replace } => {
            let text = std::fs::read_to_string(&file)
                .map_err(|err| format!("{}: {err}", file.display()))?;
            let bundle: rules::Bundle = serde_json::from_str(&text)
                .map_err(|err| format!("{} is not a Blurman export: {err}", file.display()))?;
            let mut store = rules::load();
            let mut settings = rules::load_settings();
            if replace {
                store.rules.clear();
                settings.presets.clear();
            }
            let (rule_count, preset_count) = (bundle.rules.len(), bundle.presets.len());
            for rule in bundle.rules {
                store.upsert(rule);
            }
            for preset in bundle.presets {
                settings.save_preset(preset);
            }
            store.normalize();
            rules::save(&store)?;
            rules::save_settings(&settings)?;
            ipc::signal(ipc::msg_reload());
            ipc::signal(ipc::msg_settings());
            println!("Imported {rule_count} rules and {preset_count} presets.");
        }
        Command::Clear { process, all } => {
            let mut store = rules::load();
            match process {
                Some(process) if !all => {
                    if !store.remove(&process) {
                        println!("{} had no rule.", mapping::normalize_process(&process));
                    }
                }
                _ => store.clear(),
            }
            rules::save(&store)?;
            if !ipc::signal(ipc::msg_reload()) {
                worker::restore_leftovers();
            }
            println!("Restored.");
        }
    }
    Ok(())
}

/// "chrome.exe: frost, 30% see-through, blur 40, tint #181818 at 20%, solid while focused".
fn describe(rule: &rules::Rule) -> String {
    let look = &rule.look;
    let mut parts = vec![
        look.style.to_string(),
        format!("{}% see-through", look.transparency),
        match look.style {
            BlurStyle::Frost => format!("blur {}", look.blur),
            BlurStyle::Acrylic => format!("milkiness {}", look.blur),
        },
    ];
    let finish = &look.finish;
    let tint = match finish.tint_source {
        TintSource::Custom => finish.tint.hex(),
        TintSource::Theme => "theme".to_string(),
        TintSource::Accent => "accent".to_string(),
    };
    if finish.tint_amount > 0 || finish.tint_source != TintSource::Custom {
        parts.push(format!("tint {tint} at {}%", finish.tint_amount));
    }
    if finish.saturation != mapping::SATURATION_DEFAULT {
        parts.push(format!("saturation {}%", finish.saturation));
    }
    if finish.grain > 0 {
        parts.push(format!("grain {}", finish.grain));
    }
    match rule.focus {
        FocusLook::Same => {}
        FocusLook::Lighter => parts.push("lighter while focused".into()),
        FocusLook::Solid => parts.push("solid while focused".into()),
    }
    if !rule.only_titles.is_empty() {
        parts.push(format!(
            "only \"{}\"",
            rules::join_titles(&rule.only_titles)
        ));
    }
    if !rule.skip_titles.is_empty() {
        parts.push(format!(
            "skipping \"{}\"",
            rules::join_titles(&rule.skip_titles)
        ));
    }
    format!("{}: {}", rule.process, parts.join(", "))
}

fn launch(startup: bool) {
    // Two launches at once would both miss the other's host window and both frost the apps.
    let Some(_instance) = ipc::claim_instance() else {
        if !startup {
            ipc::show_when_ready(Duration::from_secs(5));
        }
        return;
    };
    autostart::refresh();
    let shared = Shared::new();
    update::watch(shared.clone());
    let worker_shared = shared.clone();
    let worker = std::thread::Builder::new()
        .name("blurman-glass".into())
        .spawn(move || worker::run(worker_shared))
        .expect("effect thread");
    let result = app::run(shared.clone(), startup);
    shared.shutdown_worker(Duration::from_secs(3));
    if shared.worker_done.load(std::sync::atomic::Ordering::SeqCst) {
        let _ = worker.join();
    }
    if let Err(err) = result {
        ensure_console();
        eprintln!("Blurman could not open its window: {err}");
        std::process::exit(1);
    }
}

fn ensure_console() {
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS).is_err() {
            let _ = AllocConsole();
        }
    }
}
