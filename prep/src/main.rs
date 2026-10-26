//! `ddr_world_hook_prep` entry point — see `lib.rs`.

use std::process::ExitCode;

use ddr_world_hook_prep::{cli, console, dry_run, prep, EXIT_OK};

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let code = match cli::parse(&argv) {
        Ok(cli::Command::Help) => {
            print!("{}", cli::USAGE);
            EXIT_OK
        }
        Ok(cli::Command::DryRun(rest)) => dry_run::run(&rest).clamp(0, 255) as u8,
        Ok(cli::Command::Prep(args)) => {
            let code = prep(&argv, &args);
            if console::owns_console_alone() {
                console::wait_for_enter();
            }
            code
        }
        Err(e) => {
            eprintln!("error: {e}\n\n{}", cli::USAGE);
            EXIT_OK
        }
    };
    ExitCode::from(code)
}
