//! Manual/local smoke test of the exact app sandbox launcher.
use overtalk_worker_sandbox::Sandbox;
use std::{path::PathBuf, process::Stdio};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let mut sandbox = Sandbox::new(true)?;
    let mut program = None;
    while let Some(value) = args.next() {
        match value.to_str() {
            Some("--read") => sandbox.read(PathBuf::from(args.next().ok_or("path")?))?,
            Some("--write") => sandbox.output(&PathBuf::from(args.next().ok_or("path")?))?,
            Some("--cwd") => sandbox.cwd(PathBuf::from(args.next().ok_or("path")?))?,
            Some("--") => {
                program = args.next().map(PathBuf::from);
                break;
            }
            _ => return Err("Use --read PATH --cwd PATH -- PROGRAM [ARGS]".into()),
        }
    }
    let program = program.ok_or("program")?;
    let mut command = if let Some(wrapper) = overtalk_worker_sandbox::gpu_launcher() {
        sandbox.command_with_wrapper(&program, &wrapper)?
    } else {
        sandbox.command(&program)?
    };
    let status = command
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    if !status.success() {
        return Err(format!("worker exited: {status}").into());
    }
    Ok(())
}
