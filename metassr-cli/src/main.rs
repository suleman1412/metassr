mod cli;
use clap::Parser;
use cli::{
    traits::{AsyncExec, Exec},
    Args, Commands, DebugMode,
};
use logger::LoggingLayer;

use anyhow::Result;

use std::{
    env::{current_dir, set_current_dir, set_var},
    path::Path,
};

use tracing_subscriber::{
    prelude::__tracing_subscriber_SubscriberExt, util::SubscriberInitExt, EnvFilter,
};

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let allow_metacall_debug =
        [Some(DebugMode::All), Some(DebugMode::Metacall)].contains(&args.debug_mode);
    let allow_http_debug = [Some(DebugMode::All), Some(DebugMode::Http)].contains(&args.debug_mode);
    
    let tracing_level = match args.debug_mode{
        Some(DebugMode::All) => "debug",
        Some(DebugMode::Http) => "http=debug,info",
        _ => "info"
    };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(tracing_level).add_directive("notify=off".parse().unwrap())
    });
    if let Commands::Create { .. } = args.commands {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(false)
            .without_time()
            .compact()
            .init();
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(LoggingLayer {
                logfile: args.log_file,
            })
            .init();
        let project_root = Path::new(&args.root);

        set_current_dir(project_root)
            .map_err(|err| eprintln!("Cannot chdir: {err}"))
            .unwrap();

        if allow_metacall_debug {
            set_var("METACALL_DEBUG", "1");
        }
    }
    match args.commands {
        Commands::Build {
            out_dir,
            build_type,
        } => {
            tracing::info!("command build Out dir: {:?}", out_dir);

            cli::Builder::new(build_type, out_dir).exec()?;
        }
        Commands::Run { port, serve } => {
            cli::Runner::new(port, serve, allow_http_debug)
                .exec()
                .await?;
        }
        Commands::Create {
            project_name,
            version,
            description,
            template,
        } => {
            cli::Creator::new(project_name, version, description, template)?.exec()?;
        }
        Commands::Dev { port, ws_port } => {
            cli::Dev::new(
                port,
                ws_port,
                current_dir()?,
                metassr_build::server::BuildingType::ServerSideRendering,
            )?
            .exec()
            .await?;
        }
    };

    Ok(())
}
