use std::{
    ffi::OsString,
    io::{Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
};

fn windows_powershell_program() -> OsString {
    let system_root =
        std::env::var_os("SystemRoot").unwrap_or_else(|| OsString::from("C:\\Windows"));
    let powershell_path = PathBuf::from(system_root)
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    if powershell_path.exists() {
        powershell_path.into_os_string()
    } else {
        OsString::from("powershell.exe")
    }
}

fn windows_powershell_args() -> impl Iterator<Item = &'static str> {
    ["-NoLogo", "-NoProfile"].into_iter()
}

fn shell_command_for_script(script: &str) -> Command {
    if cfg!(target_os = "windows") {
        let mut command = Command::new(windows_powershell_program());
        command.args(windows_powershell_args());
        command.args(["-Command", script]);
        command
    } else {
        let mut command = Command::new("/bin/bash");
        command.args(["-lc", script]);
        command
    }
}

fn main() {
    let mut args = std::env::args();
    let _program = args.next();
    let marker = args.next().unwrap_or_else(|| "__VELOCITY_EXIT__".to_owned());
    let block_id = args.next().unwrap_or_else(|| "missing".to_owned());
    let command = args.collect::<Vec<_>>().join(" ");

    if command.trim().is_empty() {
        eprintln!("Usage: velocity-engine-runner <marker> <blockId> <command...>");
        std::process::exit(2);
    }

    let mut child = match shell_command_for_script(&command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            eprintln!("{error}");
            println!("{marker}{block_id}__1__");
            std::process::exit(1);
        }
    };

    let mut stdout = String::new();
    let mut stderr = String::new();
    if let Some(mut out) = child.stdout.take() {
        let mut buf = Vec::new();
        let _ = out.read_to_end(&mut buf);
        stdout = String::from_utf8_lossy(&buf).to_string();
    }
    if let Some(mut err) = child.stderr.take() {
        let mut buf = Vec::new();
        let _ = err.read_to_end(&mut buf);
        stderr = String::from_utf8_lossy(&buf).to_string();
    }

    let status = child.wait().ok();
    let exit_code = status.and_then(|s| s.code()).unwrap_or(1);

    let mut combined = String::new();
    combined.push_str(&stdout);
    combined.push_str(&stderr);

    let cwd = std::env::current_dir()
        .map(|dir| dir.display().to_string())
        .unwrap_or_else(|_| String::new());
    if !cwd.is_empty() {
        combined.push_str(&format!("changed directory to {cwd}\n"));
    }
    combined.push_str(&format!("{marker}{block_id}__{exit_code}__\n"));

    let _ = std::io::stdout().write_all(combined.as_bytes());
    let _ = std::io::stdout().flush();
    std::process::exit(exit_code);
}

